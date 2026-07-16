use super::{js_error, window};
use crate::{WebHostError, WebHostResult};
use boon_document::{SemanticDomNode, SemanticWebBridgeSnapshot, SemanticWebInputEvent};
use boon_host::{ImeInputKind, SemanticId};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{CompositionEvent, Element, Event, EventTarget, HtmlElement, HtmlInputElement};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticDomEvent {
    Action(SemanticWebInputEvent),
    SensitiveTextInput {
        semantic_id: SemanticId,
        text: String,
    },
    Ime {
        semantic_id: SemanticId,
        kind: ImeInputKind,
    },
    Rejected {
        error: WebHostError,
    },
}

struct DomListener {
    target: EventTarget,
    event: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for DomListener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(
            self.event,
            self.callback.as_ref().unchecked_ref(),
        );
    }
}

/// Keyed semantic DOM projection. Existing elements remain mounted across
/// updates; only attributes, text, order, and removed nodes are patched.
pub struct SemanticDomProjector {
    root: HtmlElement,
    elements: BTreeMap<SemanticId, Element>,
    attribute_names: BTreeMap<SemanticId, BTreeSet<String>>,
    listeners: Vec<DomListener>,
}

impl SemanticDomProjector {
    pub fn mount(
        parent: &Element,
        max_text_bytes: usize,
        sink: Rc<dyn Fn(SemanticDomEvent)>,
    ) -> WebHostResult<Self> {
        if max_text_bytes == 0 {
            return Err(WebHostError::InvalidInput {
                field: "semantic DOM max_text_bytes".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        let document = window()?
            .document()
            .ok_or_else(|| WebHostError::unsupported("Document", "browser document is absent"))?;
        let root = document
            .create_element("div")
            .map_err(|error| js_error("create semantic projection root", error))?
            .dyn_into::<HtmlElement>()
            .map_err(|error| js_error("cast semantic projection root", error))?;
        root.set_attribute("data-boon-semantic-root", "true")
            .map_err(|error| js_error("mark semantic projection root", error))?;
        root.set_attribute("aria-live", "off")
            .map_err(|error| js_error("configure semantic projection root", error))?;
        root.style()
            .set_css_text("position:fixed;left:0;top:0;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);pointer-events:none;contain:strict;");
        parent
            .append_child(&root)
            .map_err(|error| js_error("mount semantic projection root", error))?;

        let target: EventTarget = root.clone().into();
        let mut listeners = Vec::new();
        listeners.push(delegated_listener(
            &target,
            "click",
            max_text_bytes,
            Rc::clone(&sink),
            handle_click,
        )?);
        listeners.push(delegated_listener(
            &target,
            "focusin",
            max_text_bytes,
            Rc::clone(&sink),
            handle_focus,
        )?);
        listeners.push(delegated_listener(
            &target,
            "focusout",
            max_text_bytes,
            Rc::clone(&sink),
            handle_blur,
        )?);
        listeners.push(delegated_listener(
            &target,
            "input",
            max_text_bytes,
            Rc::clone(&sink),
            handle_input,
        )?);
        listeners.push(delegated_listener(
            &target,
            "compositionstart",
            max_text_bytes,
            Rc::clone(&sink),
            |event, element, id, max, sink| {
                handle_composition(event, element, id, max, sink, "start")
            },
        )?);
        listeners.push(delegated_listener(
            &target,
            "compositionupdate",
            max_text_bytes,
            Rc::clone(&sink),
            |event, element, id, max, sink| {
                handle_composition(event, element, id, max, sink, "update")
            },
        )?);
        listeners.push(delegated_listener(
            &target,
            "compositionend",
            max_text_bytes,
            sink,
            |event, element, id, max, sink| {
                handle_composition(event, element, id, max, sink, "end")
            },
        )?);

        Ok(Self {
            root,
            elements: BTreeMap::new(),
            attribute_names: BTreeMap::new(),
            listeners,
        })
    }

    pub fn root(&self) -> &HtmlElement {
        &self.root
    }

    pub fn apply(&mut self, bridge: &SemanticWebBridgeSnapshot) -> WebHostResult<()> {
        let desired = bridge
            .dom
            .nodes
            .iter()
            .map(|node| node.semantic_id.clone())
            .collect::<BTreeSet<_>>();
        let stale = self
            .elements
            .keys()
            .filter(|id| !desired.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for id in stale {
            if let Some(element) = self.elements.remove(&id) {
                element.remove();
            }
            self.attribute_names.remove(&id);
        }

        for node in &bridge.dom.nodes {
            let element = match self.elements.get(&node.semantic_id) {
                Some(existing) if existing.tag_name().eq_ignore_ascii_case(&node.tag) => {
                    existing.clone()
                }
                Some(existing) => {
                    let replacement = create_semantic_element(node)?;
                    existing
                        .replace_with_with_node_1(&replacement)
                        .map_err(|error| js_error("replace semantic DOM node", error))?;
                    self.elements
                        .insert(node.semantic_id.clone(), replacement.clone());
                    replacement
                }
                None => {
                    let element = create_semantic_element(node)?;
                    self.elements
                        .insert(node.semantic_id.clone(), element.clone());
                    element
                }
            };
            patch_semantic_element(
                &element,
                node,
                self.attribute_names
                    .entry(node.semantic_id.clone())
                    .or_default(),
            )?;
            self.root
                .append_child(&element)
                .map_err(|error| js_error("order semantic DOM node", error))?;
        }
        if let Some(focused) = bridge
            .dom
            .nodes
            .iter()
            .find(|node| node.attributes.contains_key("data-boon-focused"))
            .and_then(|node| self.elements.get(&node.semantic_id))
            .and_then(|element| element.dyn_ref::<HtmlElement>())
        {
            let document = window()?.document().ok_or_else(|| {
                WebHostError::unsupported("Document", "browser document is absent")
            })?;
            let already_focused = document
                .active_element()
                .is_some_and(|active| active == **focused);
            if !already_focused {
                focused
                    .focus()
                    .map_err(|error| js_error("focus semantic DOM endpoint", error))?;
            }
        }
        Ok(())
    }

    pub fn mounted_node_count(&self) -> usize {
        self.elements.len()
    }

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }
}

fn create_semantic_element(node: &SemanticDomNode) -> WebHostResult<Element> {
    window()?
        .document()
        .ok_or_else(|| WebHostError::unsupported("Document", "browser document is absent"))?
        .create_element(&node.tag)
        .map_err(|error| js_error("create semantic DOM node", error))
}

fn patch_semantic_element(
    element: &Element,
    node: &SemanticDomNode,
    previous_attributes: &mut BTreeSet<String>,
) -> WebHostResult<()> {
    let mut next_attributes = node.attributes.keys().cloned().collect::<BTreeSet<_>>();
    if node.role.is_some() {
        next_attributes.insert("role".to_owned());
    }
    for name in previous_attributes.difference(&next_attributes) {
        element
            .remove_attribute(name)
            .map_err(|error| js_error("remove semantic DOM attribute", error))?;
    }
    if let Some(role) = &node.role {
        element
            .set_attribute("role", role)
            .map_err(|error| js_error("set semantic DOM role", error))?;
    }
    for (name, value) in &node.attributes {
        element
            .set_attribute(name, value)
            .map_err(|error| js_error("set semantic DOM attribute", error))?;
    }
    if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
        input.set_value(
            node.attributes
                .get("value")
                .map(String::as_str)
                .unwrap_or_default(),
        );
        input.set_checked(node.attributes.contains_key("checked"));
    } else {
        element.set_text_content(node.text.as_deref());
    }
    *previous_attributes = next_attributes;
    Ok(())
}

type DelegatedHandler = fn(&Event, &Element, SemanticId, usize, &Rc<dyn Fn(SemanticDomEvent)>);

fn delegated_listener(
    target: &EventTarget,
    event_name: &'static str,
    max_text_bytes: usize,
    sink: Rc<dyn Fn(SemanticDomEvent)>,
    handler: DelegatedHandler,
) -> WebHostResult<DomListener> {
    let callback = Closure::wrap(Box::new(move |event: Event| {
        let Some(element) = semantic_event_element(&event) else {
            return;
        };
        let Some(id) = element.get_attribute("data-boon-id") else {
            return;
        };
        handler(&event, &element, SemanticId(id), max_text_bytes, &sink);
    }) as Box<dyn FnMut(Event)>);
    target
        .add_event_listener_with_callback(event_name, callback.as_ref().unchecked_ref())
        .map_err(|error| js_error("install semantic DOM listener", error))?;
    Ok(DomListener {
        target: target.clone(),
        event: event_name,
        callback,
    })
}

fn semantic_event_element(event: &Event) -> Option<Element> {
    let mut current = event.target()?.dyn_into::<Element>().ok()?;
    loop {
        if current.has_attribute("data-boon-id") {
            return Some(current);
        }
        current = current.parent_element()?;
    }
}

fn handle_click(
    event: &Event,
    element: &Element,
    semantic_id: SemanticId,
    _max_text_bytes: usize,
    sink: &Rc<dyn Fn(SemanticDomEvent)>,
) {
    let action = if element.has_attribute("data-boon-action-press") {
        Some(SemanticWebInputEvent::Press { semantic_id })
    } else if element.has_attribute("data-boon-action-increment") {
        Some(SemanticWebInputEvent::Increment { semantic_id })
    } else if element.has_attribute("data-boon-action-decrement") {
        Some(SemanticWebInputEvent::Decrement { semantic_id })
    } else {
        None
    };
    if let Some(action) = action {
        event.prevent_default();
        sink(SemanticDomEvent::Action(action));
    }
}

fn handle_focus(
    _event: &Event,
    _element: &Element,
    semantic_id: SemanticId,
    _max_text_bytes: usize,
    sink: &Rc<dyn Fn(SemanticDomEvent)>,
) {
    sink(SemanticDomEvent::Action(SemanticWebInputEvent::Focus {
        semantic_id,
    }));
}

fn handle_blur(
    _event: &Event,
    _element: &Element,
    semantic_id: SemanticId,
    _max_text_bytes: usize,
    sink: &Rc<dyn Fn(SemanticDomEvent)>,
) {
    sink(SemanticDomEvent::Ime {
        semantic_id,
        kind: ImeInputKind::Disabled,
    });
}

fn handle_input(
    _event: &Event,
    element: &Element,
    semantic_id: SemanticId,
    max_text_bytes: usize,
    sink: &Rc<dyn Fn(SemanticDomEvent)>,
) {
    if !element.has_attribute("data-boon-action-set-text") {
        return;
    }
    let Some(input) = element.dyn_ref::<HtmlInputElement>() else {
        return;
    };
    let text = input.value();
    if text.len() > max_text_bytes {
        sink(SemanticDomEvent::Rejected {
            error: WebHostError::LimitExceeded {
                resource: "semantic text input".to_owned(),
                limit: max_text_bytes,
            },
        });
        return;
    }
    if element.has_attribute("data-boon-sensitive-input") {
        sink(SemanticDomEvent::SensitiveTextInput { semantic_id, text });
    } else {
        sink(SemanticDomEvent::Action(SemanticWebInputEvent::SetText {
            semantic_id,
            text,
        }));
    }
}

fn handle_composition(
    event: &Event,
    _element: &Element,
    semantic_id: SemanticId,
    max_text_bytes: usize,
    sink: &Rc<dyn Fn(SemanticDomEvent)>,
    phase: &str,
) {
    let Some(composition) = event.dyn_ref::<CompositionEvent>() else {
        return;
    };
    if composition
        .data()
        .is_some_and(|text| text.len() > max_text_bytes)
    {
        sink(SemanticDomEvent::Rejected {
            error: WebHostError::LimitExceeded {
                resource: "semantic IME text".to_owned(),
                limit: max_text_bytes,
            },
        });
        return;
    }
    let kind = match phase {
        "start" => ImeInputKind::Enabled,
        "update" => ImeInputKind::Preedit {
            text: composition.data().unwrap_or_default(),
            cursor: None,
        },
        "end" => ImeInputKind::Commit {
            text: composition.data().unwrap_or_default(),
        },
        _ => return,
    };
    sink(SemanticDomEvent::Ime { semantic_id, kind });
}
