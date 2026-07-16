use super::{js_error, window};
use crate::{
    BrowserClipboardEvent, BrowserGestureEvent, BrowserHostEvent, BrowserInputNormalizer,
    BrowserLifecycleEvent, WebHostError, WebHostResult,
};
use boon_host::{ImeInputKind, PointerButton, PointerPhase, SurfaceId, WindowId};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    AddEventListenerOptions, ClipboardEvent, CompositionEvent, Event, EventTarget,
    HtmlCanvasElement, KeyboardEvent, PageTransitionEvent, PointerEvent, WheelEvent,
};

struct BrowserListener {
    target: EventTarget,
    event: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for BrowserListener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(
            self.event,
            self.callback.as_ref().unchecked_ref(),
        );
    }
}

#[derive(Default)]
struct TouchState {
    points: BTreeMap<i32, (f32, f32)>,
    previous_distance: Option<f32>,
}

pub struct BrowserInputBindings {
    canvas: HtmlCanvasElement,
    normalizer: Rc<RefCell<BrowserInputNormalizer>>,
    listeners: Vec<BrowserListener>,
}

impl BrowserInputBindings {
    pub fn install(
        canvas: HtmlCanvasElement,
        surface: SurfaceId,
        window_id: WindowId,
        max_text_bytes: usize,
        sink: Rc<dyn Fn(BrowserHostEvent)>,
    ) -> WebHostResult<Self> {
        canvas.set_tab_index(0);
        let normalizer = Rc::new(RefCell::new(BrowserInputNormalizer::new(
            surface,
            window_id,
            1,
            max_text_bytes,
        )?));
        let touch = Rc::new(RefCell::new(TouchState::default()));
        let canvas_target: EventTarget = canvas.clone().into();
        let browser_window = window()?;
        let window_target: EventTarget = browser_window.clone().into();
        let document = browser_window
            .document()
            .ok_or_else(|| WebHostError::unsupported("Document", "browser document is absent"))?;
        let document_target: EventTarget = document.clone().into();
        let mut listeners = Vec::new();

        for (event_name, phase) in [
            ("pointermove", PointerPhase::Move),
            ("pointerdown", PointerPhase::Down),
            ("pointerup", PointerPhase::Up),
            ("pointercancel", PointerPhase::Up),
            ("pointerleave", PointerPhase::Leave),
        ] {
            let canvas_for_callback = canvas.clone();
            let normalizer = Rc::clone(&normalizer);
            let sink = Rc::clone(&sink);
            let touch = Rc::clone(&touch);
            listeners.push(listener(&canvas_target, event_name, move |event| {
                let Some(pointer) = event.dyn_ref::<PointerEvent>() else {
                    return;
                };
                let (x, y) = canvas_coordinates(
                    &canvas_for_callback,
                    pointer.client_x(),
                    pointer.client_y(),
                );
                if pointer.pointer_type() == "touch" {
                    emit_touch_gesture(
                        &sink,
                        &mut touch.borrow_mut(),
                        pointer.pointer_id(),
                        x,
                        y,
                        phase,
                    );
                }
                let button = pointer_button(pointer.button());
                emit_result(&sink, normalizer.borrow_mut().pointer(x, y, phase, button));
                if matches!(phase, PointerPhase::Down) {
                    let _ = canvas_for_callback.set_pointer_capture(pointer.pointer_id());
                    let _ = canvas_for_callback.focus();
                } else if matches!(phase, PointerPhase::Up | PointerPhase::Leave) {
                    let _ = canvas_for_callback.release_pointer_capture(pointer.pointer_id());
                }
            })?);
        }

        {
            let canvas_for_callback = canvas.clone();
            let normalizer = Rc::clone(&normalizer);
            let sink = Rc::clone(&sink);
            listeners.push(listener_with_options(
                &canvas_target,
                "wheel",
                false,
                move |event| {
                    let Some(wheel) = event.dyn_ref::<WheelEvent>() else {
                        return;
                    };
                    wheel.prevent_default();
                    let (x, y) = canvas_coordinates(
                        &canvas_for_callback,
                        wheel.client_x(),
                        wheel.client_y(),
                    );
                    let factor = match wheel.delta_mode() {
                        1 => 16.0,
                        2 => f64::from(canvas_for_callback.client_height().max(1)),
                        _ => 1.0,
                    };
                    emit_result(
                        &sink,
                        normalizer.borrow_mut().wheel(
                            x,
                            y,
                            (wheel.delta_x() * factor) as f32,
                            (wheel.delta_y() * factor) as f32,
                        ),
                    );
                },
            )?);
        }

        for (event_name, pressed) in [("keydown", true), ("keyup", false)] {
            let normalizer = Rc::clone(&normalizer);
            let sink = Rc::clone(&sink);
            listeners.push(listener(&canvas_target, event_name, move |event| {
                let Some(keyboard) = event.dyn_ref::<KeyboardEvent>() else {
                    return;
                };
                emit_result(
                    &sink,
                    normalizer
                        .borrow_mut()
                        .key(Some(keyboard.code()), keyboard.key(), pressed),
                );
            })?);
        }

        {
            let normalizer = Rc::clone(&normalizer);
            let sink = Rc::clone(&sink);
            listeners.push(listener(&canvas_target, "focus", move |_event| {
                sink(normalizer.borrow_mut().focus(true));
            })?);
        }
        {
            let normalizer = Rc::clone(&normalizer);
            let sink = Rc::clone(&sink);
            listeners.push(listener(&canvas_target, "blur", move |_event| {
                sink(normalizer.borrow_mut().focus(false));
            })?);
        }

        for (event_name, phase) in [
            ("compositionstart", "start"),
            ("compositionupdate", "update"),
            ("compositionend", "end"),
        ] {
            let normalizer = Rc::clone(&normalizer);
            let sink = Rc::clone(&sink);
            listeners.push(listener(&canvas_target, event_name, move |event| {
                let Some(composition) = event.dyn_ref::<CompositionEvent>() else {
                    return;
                };
                let data = composition.data().unwrap_or_default();
                let kind = match phase {
                    "start" => ImeInputKind::Enabled,
                    "update" => ImeInputKind::Preedit {
                        text: data,
                        cursor: None,
                    },
                    "end" => ImeInputKind::Commit { text: data },
                    _ => return,
                };
                emit_result(&sink, normalizer.borrow_mut().ime(kind));
            })?);
        }

        for event_name in ["copy", "cut", "paste"] {
            let sink = Rc::clone(&sink);
            listeners.push(listener(&canvas_target, event_name, move |event| {
                let clipboard_event = match event_name {
                    "copy" => BrowserClipboardEvent::CopyRequested,
                    "cut" => BrowserClipboardEvent::CutRequested,
                    "paste" => event
                        .dyn_ref::<ClipboardEvent>()
                        .and_then(ClipboardEvent::clipboard_data)
                        .and_then(|data| data.get_data("text/plain").ok())
                        .map(|text| BrowserClipboardEvent::PasteText { text })
                        .unwrap_or_else(|| BrowserClipboardEvent::ReadDenied {
                            reason: "clipboard text was unavailable".to_owned(),
                        }),
                    _ => return,
                };
                sink(BrowserHostEvent::Clipboard {
                    event: clipboard_event,
                });
            })?);
        }

        {
            let canvas_for_callback = canvas.clone();
            let normalizer = Rc::clone(&normalizer);
            let sink = Rc::clone(&sink);
            listeners.push(listener(&window_target, "resize", move |_event| {
                emit_resize(&canvas_for_callback, &normalizer, &sink);
            })?);
        }

        {
            let document_for_callback = document.clone();
            let sink = Rc::clone(&sink);
            listeners.push(listener(
                &document_target,
                "visibilitychange",
                move |_event| {
                    sink(BrowserHostEvent::Lifecycle {
                        event: BrowserLifecycleEvent::VisibilityChanged {
                            visible: document_for_callback.visibility_state()
                                == web_sys::VisibilityState::Visible,
                        },
                    });
                },
            )?);
        }

        for (event_name, online) in [("online", true), ("offline", false)] {
            let sink = Rc::clone(&sink);
            listeners.push(listener(&window_target, event_name, move |_event| {
                sink(BrowserHostEvent::Lifecycle {
                    event: BrowserLifecycleEvent::OnlineChanged { online },
                });
            })?);
        }

        for (event_name, show) in [("pagehide", false), ("pageshow", true)] {
            let sink = Rc::clone(&sink);
            listeners.push(listener(&window_target, event_name, move |event| {
                let persisted = event
                    .dyn_ref::<PageTransitionEvent>()
                    .is_some_and(PageTransitionEvent::persisted);
                sink(BrowserHostEvent::Lifecycle {
                    event: if show {
                        BrowserLifecycleEvent::PageShow { persisted }
                    } else {
                        BrowserLifecycleEvent::PageHide { persisted }
                    },
                });
            })?);
        }

        {
            let sink = Rc::clone(&sink);
            listeners.push(listener(&window_target, "beforeunload", move |_event| {
                sink(BrowserHostEvent::Lifecycle {
                    event: BrowserLifecycleEvent::BeforeUnload,
                });
            })?);
        }

        {
            let sink = Rc::clone(&sink);
            listeners.push(listener(&window_target, "popstate", move |_event| {
                match current_path_query_fragment() {
                    Ok(path_query_fragment) => sink(BrowserHostEvent::UrlChanged {
                        path_query_fragment,
                    }),
                    Err(error) => sink(BrowserHostEvent::Rejected { error }),
                }
            })?);
        }

        if let Some(query) = browser_window
            .match_media("(prefers-reduced-motion: reduce)")
            .map_err(|error| js_error("query reduced motion", error))?
        {
            sink(BrowserHostEvent::Lifecycle {
                event: BrowserLifecycleEvent::ReducedMotionChanged {
                    reduced: query.matches(),
                },
            });
            let query_for_callback = query.clone();
            let sink = Rc::clone(&sink);
            let target: EventTarget = query.into();
            listeners.push(listener(&target, "change", move |_event| {
                sink(BrowserHostEvent::Lifecycle {
                    event: BrowserLifecycleEvent::ReducedMotionChanged {
                        reduced: query_for_callback.matches(),
                    },
                });
            })?);
        }

        emit_resize(&canvas, &normalizer, &sink);
        sink(BrowserHostEvent::Lifecycle {
            event: BrowserLifecycleEvent::OnlineChanged {
                online: browser_window.navigator().on_line(),
            },
        });
        sink(BrowserHostEvent::Lifecycle {
            event: BrowserLifecycleEvent::VisibilityChanged {
                visible: document.visibility_state() == web_sys::VisibilityState::Visible,
            },
        });

        Ok(Self {
            canvas,
            normalizer,
            listeners,
        })
    }

    pub fn canvas(&self) -> &HtmlCanvasElement {
        &self.canvas
    }

    pub fn surface_epoch(&self) -> u64 {
        self.normalizer.borrow().surface_epoch()
    }

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }
}

fn listener(
    target: &EventTarget,
    event: &'static str,
    callback: impl FnMut(&Event) + 'static,
) -> WebHostResult<BrowserListener> {
    listener_with_options(target, event, true, callback)
}

fn listener_with_options(
    target: &EventTarget,
    event: &'static str,
    passive: bool,
    mut callback: impl FnMut(&Event) + 'static,
) -> WebHostResult<BrowserListener> {
    let closure =
        Closure::wrap(Box::new(move |event: Event| callback(&event)) as Box<dyn FnMut(Event)>);
    let options = AddEventListenerOptions::new();
    options.set_passive(passive);
    target
        .add_event_listener_with_callback_and_add_event_listener_options(
            event,
            closure.as_ref().unchecked_ref(),
            &options,
        )
        .map_err(|error| js_error("install browser input listener", error))?;
    Ok(BrowserListener {
        target: target.clone(),
        event,
        callback: closure,
    })
}

fn canvas_coordinates(canvas: &HtmlCanvasElement, client_x: i32, client_y: i32) -> (f32, f32) {
    let bounds = canvas.get_bounding_client_rect();
    (
        (f64::from(client_x) - bounds.left()) as f32,
        (f64::from(client_y) - bounds.top()) as f32,
    )
}

fn pointer_button(button: i16) -> Option<PointerButton> {
    match button {
        -1 => None,
        0 => Some(PointerButton::Primary),
        1 => Some(PointerButton::Middle),
        2 => Some(PointerButton::Secondary),
        value if (0..=u8::MAX as i16).contains(&value) => Some(PointerButton::Other(value as u8)),
        _ => None,
    }
}

fn emit_touch_gesture(
    sink: &Rc<dyn Fn(BrowserHostEvent)>,
    touch: &mut TouchState,
    pointer_id: i32,
    x: f32,
    y: f32,
    phase: PointerPhase,
) {
    let event = match phase {
        PointerPhase::Down => {
            touch.points.insert(pointer_id, (x, y));
            BrowserGestureEvent::TouchStart { pointer_id, x, y }
        }
        PointerPhase::Move => {
            touch.points.insert(pointer_id, (x, y));
            BrowserGestureEvent::TouchMove { pointer_id, x, y }
        }
        PointerPhase::Up | PointerPhase::Leave => {
            touch.points.remove(&pointer_id);
            touch.previous_distance = None;
            BrowserGestureEvent::TouchEnd { pointer_id, x, y }
        }
    };
    sink(BrowserHostEvent::Gesture { event });
    if touch.points.len() == 2 {
        let mut points = touch.points.values();
        let (x1, y1) = points.next().copied().unwrap_or_default();
        let (x2, y2) = points.next().copied().unwrap_or_default();
        let distance = (x2 - x1).hypot(y2 - y1).max(0.001);
        if let Some(previous) = touch.previous_distance
            && previous > 0.0
        {
            sink(BrowserHostEvent::Gesture {
                event: BrowserGestureEvent::Pinch {
                    center_x: (x1 + x2) * 0.5,
                    center_y: (y1 + y2) * 0.5,
                    scale_delta: distance / previous,
                },
            });
        }
        touch.previous_distance = Some(distance);
    }
}

fn emit_resize(
    canvas: &HtmlCanvasElement,
    normalizer: &Rc<RefCell<BrowserInputNormalizer>>,
    sink: &Rc<dyn Fn(BrowserHostEvent)>,
) {
    let Ok(browser_window) = window() else {
        return;
    };
    let logical_width = canvas.client_width().max(1) as f32;
    let logical_height = canvas.client_height().max(1) as f32;
    let scale = browser_window.device_pixel_ratio();
    let physical_width = (f64::from(logical_width) * scale)
        .round()
        .clamp(1.0, u32::MAX as f64) as u32;
    let physical_height = (f64::from(logical_height) * scale)
        .round()
        .clamp(1.0, u32::MAX as f64) as u32;
    emit_result(
        sink,
        normalizer.borrow_mut().resize(
            logical_width,
            logical_height,
            scale,
            physical_width,
            physical_height,
        ),
    );
}

fn emit_result(sink: &Rc<dyn Fn(BrowserHostEvent)>, result: WebHostResult<BrowserHostEvent>) {
    match result {
        Ok(event) => sink(event),
        Err(error) => sink(BrowserHostEvent::Rejected { error }),
    }
}

fn current_path_query_fragment() -> WebHostResult<String> {
    let location = window()?.location();
    let path = location
        .pathname()
        .map_err(|error| js_error("read location pathname", error))?;
    let query = location
        .search()
        .map_err(|error| js_error("read location search", error))?;
    let fragment = location
        .hash()
        .map_err(|error| js_error("read location hash", error))?;
    Ok(format!("{path}{query}{fragment}"))
}
