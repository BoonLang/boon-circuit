use boon_document::{
    DocumentNodeId, MapCamera, MapHitIdentity, MapScreenPoint, MapViewportDescriptor,
    MapViewportDescriptorPatch, MapViewportError, MapViewportInput, Rect, RenderScene,
    RenderScenePatch, RenderScenePatchOperation,
};
use boon_host::{HostEvent, LogicalKey, PointerButton, PointerPhase};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const WHEEL_ZOOM_PER_PIXEL: f64 = 0.002;
const MAX_ZOOM_DELTA_PER_EVENT: f64 = 2.0;
const CLICK_SLOP_PIXELS: f32 = 5.0;
const MAX_PENDING_MAP_HOST_EVENTS: usize = 1_024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapViewportHostEvent {
    CameraChanged {
        node: DocumentNodeId,
        camera: MapCamera,
        generation: u64,
    },
    OverlayActivated {
        node: DocumentNodeId,
        hit_identity: MapHitIdentity,
    },
}

#[derive(Clone, Debug)]
struct ControlledMap {
    base: MapViewportDescriptor,
    current: MapViewportDescriptor,
    retained_chunk_id: String,
    bounds: Rect,
}

#[derive(Clone, Debug)]
struct DragState {
    node: DocumentNodeId,
    last_x: f32,
    last_y: f32,
    start_x: f32,
    start_y: f32,
}

/// Host-local retained map interaction state. The controller consumes only
/// generic `RenderScene` descriptors and emits camera/overlay events for the
/// owning program. It never identifies an application or tile provider.
#[derive(Clone, Debug, Default)]
pub struct MapViewportHostController {
    maps: BTreeMap<DocumentNodeId, ControlledMap>,
    drag: Option<DragState>,
    focused: Option<DocumentNodeId>,
    active_touches: BTreeMap<i32, (f32, f32)>,
    events: VecDeque<MapViewportHostEvent>,
    revision: u64,
}

impl MapViewportHostController {
    pub fn contains_map_point(&self, scene: &RenderScene, x: f32, y: f32) -> bool {
        map_at(scene, x, y).is_some()
    }

    pub fn active_touch_count(&self) -> usize {
        self.active_touches.len()
    }

    pub fn consumes_host_event(&self, scene: &RenderScene, event: &HostEvent) -> bool {
        match event {
            HostEvent::Pointer(_) if !self.active_touches.is_empty() => self.focused.is_some(),
            HostEvent::Pointer(pointer) => match pointer.phase {
                PointerPhase::Down => map_at(scene, pointer.x, pointer.y).is_some(),
                PointerPhase::Move | PointerPhase::Up | PointerPhase::Leave => self.drag.is_some(),
            },
            HostEvent::Wheel(wheel) => map_at(scene, wheel.x, wheel.y).is_some(),
            HostEvent::Keyboard(key) if key.pressed => self.focused.is_some(),
            _ => false,
        }
    }

    pub fn sync_scene(&mut self, scene: &RenderScene) {
        let live = scene
            .map_viewports
            .iter()
            .map(|map| map.node.clone())
            .collect::<BTreeSet<_>>();
        self.maps.retain(|node, _| live.contains(node));
        if self
            .focused
            .as_ref()
            .is_some_and(|node| !live.contains(node))
        {
            self.focused = None;
        }
        if self
            .drag
            .as_ref()
            .is_some_and(|drag| !live.contains(&drag.node))
        {
            self.drag = None;
        }

        for map in &scene.map_viewports {
            match self.maps.get_mut(&map.node) {
                None => {
                    self.maps.insert(
                        map.node.clone(),
                        ControlledMap {
                            base: map.descriptor.clone(),
                            current: map.descriptor.clone(),
                            retained_chunk_id: map.retained_chunk_id.clone(),
                            bounds: map.bounds,
                        },
                    );
                }
                Some(controlled) => {
                    let app_changed_camera = map.descriptor.generation
                        != controlled.base.generation
                        || map.descriptor.camera != controlled.base.camera
                        || map.descriptor.tile_source != controlled.base.tile_source;
                    if app_changed_camera {
                        controlled.current = map.descriptor.clone();
                    } else {
                        controlled
                            .current
                            .overlays
                            .clone_from(&map.descriptor.overlays);
                        controlled.current.interaction = map.descriptor.interaction;
                    }
                    controlled.base = map.descriptor.clone();
                    controlled.retained_chunk_id = map.retained_chunk_id.clone();
                    controlled.bounds = map.bounds;
                }
            }
        }
    }

    pub fn handle_host_event(
        &mut self,
        scene: &RenderScene,
        event: &HostEvent,
    ) -> Result<bool, MapViewportError> {
        self.sync_scene(scene);
        match event {
            HostEvent::Pointer(_) if !self.active_touches.is_empty() => Ok(false),
            HostEvent::Pointer(pointer) => {
                self.handle_pointer(scene, pointer.x, pointer.y, pointer.phase, pointer.button)
            }
            HostEvent::Wheel(wheel) => {
                let Some(node) = map_at(scene, wheel.x, wheel.y) else {
                    return Ok(false);
                };
                let delta = (-f64::from(wheel.delta_y) * WHEEL_ZOOM_PER_PIXEL)
                    .clamp(-MAX_ZOOM_DELTA_PER_EVENT, MAX_ZOOM_DELTA_PER_EVENT);
                if delta == 0.0 {
                    return Ok(false);
                }
                self.apply_input(
                    &node,
                    MapViewportInput::ZoomAt {
                        delta,
                        anchor: map_local_point(scene, &node, wheel.x, wheel.y),
                    },
                )
            }
            HostEvent::Keyboard(key) if key.pressed => {
                let Some(node) = self.focused.clone() else {
                    return Ok(false);
                };
                let delta = match &key.logical_key {
                    LogicalKey::Character(value) if value == "+" || value == "=" => 1.0,
                    LogicalKey::Character(value) if value == "-" || value == "_" => -1.0,
                    LogicalKey::Named(value) if value == "Add" => 1.0,
                    LogicalKey::Named(value) if value == "Subtract" => -1.0,
                    _ => return Ok(false),
                };
                self.apply_input(&node, MapViewportInput::Zoom { delta })
            }
            HostEvent::Resize(resize) => {
                let nodes = self.maps.keys().cloned().collect::<Vec<_>>();
                let mut changed = false;
                for node in nodes {
                    let Some(controlled) = self.maps.get(&node) else {
                        continue;
                    };
                    changed |= self.apply_input(
                        &node,
                        MapViewportInput::Resize {
                            bounds: boon_document::MapViewportBounds {
                                width: f64::from(controlled.bounds.width.max(1.0)),
                                height: f64::from(controlled.bounds.height.max(1.0)),
                                scale: resize.scale,
                            },
                        },
                    )?;
                }
                Ok(changed)
            }
            _ => Ok(false),
        }
    }

    pub fn touch_start(&mut self, scene: &RenderScene, pointer_id: i32, x: f32, y: f32) {
        self.sync_scene(scene);
        self.active_touches.insert(pointer_id, (x, y));
        if self.active_touches.len() == 1
            && let Some(node) = map_at(scene, x, y)
        {
            self.focused = Some(node.clone());
            self.drag = Some(DragState {
                node,
                last_x: x,
                last_y: y,
                start_x: x,
                start_y: y,
            });
        } else if self.active_touches.len() > 1 {
            self.drag = None;
        }
    }

    pub fn touch_move(
        &mut self,
        pointer_id: i32,
        x: f32,
        y: f32,
    ) -> Result<bool, MapViewportError> {
        if let Some(point) = self.active_touches.get_mut(&pointer_id) {
            *point = (x, y);
        }
        if self.active_touches.len() != 1 {
            return Ok(false);
        }
        self.drag_to(x, y)
    }

    pub fn touch_end(&mut self, pointer_id: i32) {
        self.active_touches.remove(&pointer_id);
        self.drag = None;
        if let Some((_, &(x, y))) = self.active_touches.iter().next()
            && let Some(node) = self.focused.clone()
        {
            self.drag = Some(DragState {
                node,
                last_x: x,
                last_y: y,
                start_x: x,
                start_y: y,
            });
        }
    }

    pub fn pinch(
        &mut self,
        scene: &RenderScene,
        center_x: f32,
        center_y: f32,
        scale_delta: f32,
    ) -> Result<bool, MapViewportError> {
        self.sync_scene(scene);
        if !scale_delta.is_finite() || scale_delta <= 0.0 {
            return Ok(false);
        }
        let Some(node) = map_at(scene, center_x, center_y) else {
            return Ok(false);
        };
        self.focused = Some(node.clone());
        self.apply_input(
            &node,
            MapViewportInput::ZoomAt {
                delta: f64::from(scale_delta)
                    .log2()
                    .clamp(-MAX_ZOOM_DELTA_PER_EVENT, MAX_ZOOM_DELTA_PER_EVENT),
                anchor: map_local_point(scene, &node, center_x, center_y),
            },
        )
    }

    pub fn scene_for_render(
        &mut self,
        scene: &RenderScene,
    ) -> Result<RenderScene, MapViewportError> {
        self.sync_scene(scene);
        let mut rendered = scene.clone();
        for (node, controlled) in &self.maps {
            let Some(base_map) = scene.map_viewports.iter().find(|map| map.node == *node) else {
                continue;
            };
            if controlled.current == base_map.descriptor {
                continue;
            }
            let patch = base_map.descriptor.retained_patch_to(&controlled.current)?;
            rendered
                .apply_patch(&RenderScenePatch {
                    operations: vec![RenderScenePatchOperation::MapViewport {
                        node: node.clone(),
                        patch: Box::new(patch),
                        retained_chunk_id: format!(
                            "{}:host-camera:{}",
                            controlled.retained_chunk_id, controlled.current.generation.0
                        ),
                    }],
                })
                .map_err(|error| {
                    MapViewportError::new("host_patch", format!("apply retained patch: {error}"))
                })?;
        }
        Ok(rendered)
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = MapViewportHostEvent> + '_ {
        self.events.drain(..)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn handle_pointer(
        &mut self,
        scene: &RenderScene,
        x: f32,
        y: f32,
        phase: PointerPhase,
        button: Option<PointerButton>,
    ) -> Result<bool, MapViewportError> {
        match phase {
            PointerPhase::Down if button == Some(PointerButton::Primary) => {
                let Some(node) = map_at(scene, x, y) else {
                    self.focused = None;
                    return Ok(false);
                };
                self.focused = Some(node.clone());
                self.drag = Some(DragState {
                    node,
                    last_x: x,
                    last_y: y,
                    start_x: x,
                    start_y: y,
                });
                Ok(false)
            }
            PointerPhase::Move if self.drag.is_some() => self.drag_to(x, y),
            PointerPhase::Up if button == Some(PointerButton::Primary) => {
                let drag = self.drag.take();
                let Some(drag) = drag else {
                    return Ok(false);
                };
                let moved = (x - drag.start_x).hypot(y - drag.start_y);
                if moved <= CLICK_SLOP_PIXELS
                    && let Some(map) = scene.map_viewports.iter().find(|map| map.node == drag.node)
                    && let Some(hit) = map.hit_test(x, y)
                {
                    self.push_event(MapViewportHostEvent::OverlayActivated {
                        node: drag.node,
                        hit_identity: hit.hit_identity.clone(),
                    });
                }
                Ok(false)
            }
            PointerPhase::Leave => {
                self.drag = None;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn drag_to(&mut self, x: f32, y: f32) -> Result<bool, MapViewportError> {
        let Some(mut drag) = self.drag.take() else {
            return Ok(false);
        };
        let delta_x = f64::from(x - drag.last_x);
        let delta_y = f64::from(y - drag.last_y);
        drag.last_x = x;
        drag.last_y = y;
        let node = drag.node.clone();
        self.drag = Some(drag);
        if delta_x == 0.0 && delta_y == 0.0 {
            return Ok(false);
        }
        self.apply_input(&node, MapViewportInput::PanPixels { delta_x, delta_y })
    }

    fn apply_input(
        &mut self,
        node: &DocumentNodeId,
        input: MapViewportInput,
    ) -> Result<bool, MapViewportError> {
        let Some(controlled) = self.maps.get_mut(node) else {
            return Ok(false);
        };
        let patch: MapViewportDescriptorPatch = controlled.current.patch_for_input(input)?;
        if patch.is_empty() {
            return Ok(false);
        }
        patch.apply_to(&mut controlled.current)?;
        let camera = controlled.current.camera;
        let generation = controlled.current.generation.0;
        self.revision = self.revision.saturating_add(1);
        self.push_event(MapViewportHostEvent::CameraChanged {
            node: node.clone(),
            camera,
            generation,
        });
        Ok(true)
    }

    fn push_event(&mut self, event: MapViewportHostEvent) {
        if self.events.len() == MAX_PENDING_MAP_HOST_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

fn map_at(scene: &RenderScene, x: f32, y: f32) -> Option<DocumentNodeId> {
    scene
        .map_viewports
        .iter()
        .filter(|map| rect_contains(map.bounds, x, y))
        .min_by(|left, right| rect_area(left.bounds).total_cmp(&rect_area(right.bounds)))
        .map(|map| map.node.clone())
}

fn map_local_point(scene: &RenderScene, node: &DocumentNodeId, x: f32, y: f32) -> MapScreenPoint {
    scene
        .map_viewports
        .iter()
        .find(|map| map.node == *node)
        .map(|map| MapScreenPoint {
            x: f64::from(x - map.bounds.x),
            y: f64::from(y - map.bounds.y),
        })
        .unwrap_or(MapScreenPoint {
            x: f64::from(x),
            y: f64::from(y),
        })
}

fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn rect_area(rect: Rect) -> f32 {
    rect.width.max(0.0) * rect.height.max(0.0)
}
