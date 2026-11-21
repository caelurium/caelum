struct Uniforms {
    rect: vec4<f32>,
    viewport: vec4<f32>,
    clip: vec4<f32>,
    fade: vec4<u32>,
    animation: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var cells: texture_2d<u32>;
@group(0) @binding(2) var glyphs: texture_2d<f32>;
@group(0) @binding(3) var glyph_sampler: sampler;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
}

@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let point = u.rect.xy + corner * u.rect.zw - u.viewport.zw;
    var out: Vertex;
    out.position = vec4<f32>(point / u.viewport.xy * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.local = corner * u.rect.zw;
    return out;
}

fn linear(rgb: vec3<f32>) -> vec3<f32> {
    return select(pow((rgb + 0.055) / 1.055, vec3<f32>(2.4)), rgb / 12.92, rgb <= vec3<f32>(0.04045));
}

fn srgb(rgb: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055, rgb * 12.92, rgb <= vec3<f32>(0.0031308));
}

fn glyph(index: u32, within: vec2<f32>) -> f32 {
    let at = vec2<f32>(f32(index) * 16.0, 0.0) + clamp(within * vec2<f32>(16.0, 28.0), vec2<f32>(0.5), vec2<f32>(15.5, 27.5));
    return textureSampleLevel(glyphs, glyph_sampler, at / vec2<f32>(160.0, 28.0), 0.0).r;
}

fn hash(at: vec2<u32>) -> u32 {
    var value = at.x * 0x9e3779b9u + at.y * 0x85ebca6bu + 0x632be59bu;
    value = (value ^ (value >> 16u)) * 0x7feb352du;
    value = (value ^ (value >> 15u)) * 0x846ca68bu;
    return value ^ (value >> 16u);
}

fn ink(at: vec2<i32>, within: vec2<f32>, phase: f32, background: vec3<f32>) -> vec3<f32> {
    if any(at < vec2<i32>(0)) || any(at >= vec2<i32>(300, 100)) {
        return background;
    }
    let cell = textureLoad(cells, at, 0);
    if cell.a == 0u {
        return background;
    }
    var energy = 1.0;
    if u.animation.y > 0.5 {
        let seed = hash(vec2<u32>(at));
        let offset = f32(seed & 1023u) / 1024.0 * 6.2831853;
        energy += 0.20 * sin(phase * f32(3u + ((seed >> 10u) & 3u)) + offset)
            + 0.08 * sin(phase * f32(7u + ((seed >> 12u) & 3u)) - offset);
    }
    return mix(background, linear(vec3<f32>(cell.rgb) / 255.0) * energy, glyph(cell.a, within));
}

fn sky(grid: vec2<f32>, phase: f32) -> vec3<f32> {
    let background = linear(vec3<f32>(8.0, 9.0, 11.0) / 255.0);
    let at = vec2<i32>(floor(grid));
    if any(at < vec2<i32>(0)) || any(at >= vec2<i32>(300, 100)) {
        return background;
    }
    let tile = vec2<u32>(at) / vec2<u32>(8u);
    let seed = hash(tile);
    let star = tile * vec2<u32>(8u) + vec2<u32>((seed >> 8u) & 7u, (seed >> 16u) & 7u);
    if (seed & 255u) >= 56u || any(vec2<u32>(at) != star) {
        return background;
    }
    let uv = (vec2<f32>(star) + 0.5) / vec2<f32>(300.0, 100.0);
    if length((uv - vec2<f32>(0.52, 0.46)) / vec2<f32>(0.18, 0.20)) < 1.0
        || textureLoad(cells, at, 0).a != 0u {
        return background;
    }
    let tint = f32(seed >> 24u) / 255.0;
    var energy = 0.55 + 0.35 * tint;
    if u.animation.y > 0.5 {
        energy *= 0.88 + 0.12 * sin(phase * f32(2u + ((seed >> 22u) & 3u)) + tint * 6.2831853);
    }
    let colour = mix(vec3<f32>(0.42, 0.56, 0.78), vec3<f32>(0.78, 0.76, 0.86), tint);
    let index = select(1u, 5u, ((seed >> 20u) & 15u) == 0u);
    return mix(background, linear(colour) * energy, glyph(index, fract(grid)));
}

@fragment fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let reference = vec2<f32>(2000.0, 1122.0);
    let scale = max(u.rect.z / reference.x, u.rect.w / reference.y);
    let uv = (in.local - u.rect.zw * 0.5) / (reference * scale) + 0.5;
    let grid = uv * vec2<f32>(300.0, 100.0);
    let phase = u.animation.x * 6.2831853;
    let background = sky(grid, phase);
    var colour: vec3<f32>;
    if u.animation.y > 0.5 {
        let travel = clamp(43.0 - floor(grid.y), -16.0, 16.0) * 6.0;
        let shift = u.animation.x * travel;
        let moving = grid - vec2<f32>(shift, 0.0);
        let within = fract(moving);
        let source = floor(moving) + vec2<f32>(shift, 0.0);
        let at = vec2<i32>(floor(source));
        colour = mix(ink(at, within, phase, background), ink(at + vec2<i32>(1, 0), within, phase, background), fract(source.x));
    } else {
        colour = ink(vec2<i32>(floor(grid)), fract(grid), 0.0, background);
    }
    let alpha = clip_alpha(in.position.xy + u.viewport.zw, u.clip, u.fade);
    if u.animation.z > 0.5 {
        colour = srgb(colour);
    }
    return vec4<f32>(colour * alpha, alpha);
}
