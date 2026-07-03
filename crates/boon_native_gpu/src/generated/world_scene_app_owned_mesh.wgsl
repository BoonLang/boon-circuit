struct CameraUniform {
    clip_from_world_row0: vec4<f32>,
    clip_from_world_row1: vec4<f32>,
    clip_from_world_row2: vec4<f32>,
    clip_from_world_row3: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) world_position: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) normal_color: vec4<f32>,
    @location(3) feature_color: vec4<f32>,
    @location(4) pick_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal_color: vec4<f32>,
    @location(2) feature_color: vec4<f32>,
    @location(3) pick_color: vec4<f32>,
};

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @location(1) normal_color: vec4<f32>,
    @location(2) feature_color: vec4<f32>,
    @location(3) pick_color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(
        dot(camera.clip_from_world_row0, input.world_position),
        dot(camera.clip_from_world_row1, input.world_position),
        dot(camera.clip_from_world_row2, input.world_position),
        dot(camera.clip_from_world_row3, input.world_position),
    );
    out.color = input.color;
    out.normal_color = input.normal_color;
    out.feature_color = input.feature_color;
    out.pick_color = input.pick_color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    out.color = input.color;
    out.normal_color = input.normal_color;
    out.feature_color = input.feature_color;
    out.pick_color = input.pick_color;
    return out;
}
