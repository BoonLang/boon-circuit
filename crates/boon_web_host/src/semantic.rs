use boon_document::{SemanticWebBridgeSnapshot, SemanticWebInputEvent};
use boon_host::{SemanticInputEvent, SemanticPatch, SemanticScene, SemanticSourceDispatch};

#[derive(Clone, Debug, Default)]
pub struct SemanticProjectionState {
    scene: SemanticScene,
    bridge: SemanticWebBridgeSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticProjectionUpdate {
    pub patch: SemanticPatch,
    pub bridge: SemanticWebBridgeSnapshot,
}

impl SemanticProjectionState {
    pub fn new(scene: SemanticScene) -> Self {
        let bridge = SemanticWebBridgeSnapshot::from_scene(&scene);
        Self { scene, bridge }
    }

    pub fn scene(&self) -> &SemanticScene {
        &self.scene
    }

    pub fn bridge(&self) -> &SemanticWebBridgeSnapshot {
        &self.bridge
    }

    pub fn update(&mut self, next: SemanticScene) -> SemanticProjectionUpdate {
        let patch = self.scene.diff(&next);
        let bridge = SemanticWebBridgeSnapshot::from_scene(&next);
        self.scene = next;
        self.bridge = bridge.clone();
        SemanticProjectionUpdate { patch, bridge }
    }

    pub fn source_dispatch_for_web_event(
        &self,
        event: SemanticWebInputEvent,
    ) -> Option<SemanticSourceDispatch> {
        self.bridge.source_dispatch_for_event(event)
    }

    pub fn source_dispatch_for_semantic_event(
        &self,
        event: SemanticInputEvent,
    ) -> Option<SemanticSourceDispatch> {
        self.scene.source_dispatch_for_event(event)
    }
}
