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
    // Half-texel padding keeps filtering within this glyph's cell.
    let at = vec2<f32>(f32(index) * 16.0, 0.0) + clamp(within * vec2<f32>(16.0, 28.0), vec2<f32>(0.5), vec2<f32>(15.5, 27.5));
    return textureSampleLevel(glyphs, glyph_sampler, at / vec2<f32>(160.0, 28.0), 0.0).r;
}

fn ink(at: vec2<i32>, within: vec2<f32>, phase: f32, energy: f32) -> vec3<f32> {
    let background = linear(vec3<f32>(8.0, 9.0, 11.0) / 255.0);
    if any(at < vec2<i32>(0)) || any(at >= vec2<i32>(300, 100)) {
        return background;
    }
    let cell = textureLoad(cells, at, 0);
    if cell.a == 0u {
        return background;
    }
    var coverage = glyph(cell.a, within);
    if u.animation.y > 0.5 {
        // Each character evolves smoothly at its own phase, using only the
        // supplied density alphabet. Empty source cells remain empty.
        let seed = f32((u32(at.x) * 1664525u ^ u32(at.y) * 1013904223u) & 1023u) / 1024.0 * 6.2831853;
        let density = 0.65 * sin(phase * 9.0 + seed)
            + 0.35 * sin(phase * 5.0 + dot(vec2<f32>(at), vec2<f32>(0.13, 0.25)));
        let next = select(max(cell.a, 2u) - 1u, min(cell.a + 1u, 9u), density > 0.0);
        coverage = mix(coverage, glyph(next, within), 0.85 * smoothstep(0.25, 0.9, abs(density)));
    }
    return mix(background, linear(vec3<f32>(cell.rgb) / 255.0) * energy, coverage);
}

// Bounded deformation and light energy, in source-image coordinates. The
// independent wave rates share a long cycle rather than a short uniform spin.
fn current(uv: vec2<f32>, phase: f32) -> vec3<f32> {
    let aspect = vec2<f32>(2000.0 / 1122.0, 1.0);
    let centre = vec2<f32>(0.52, 0.43);
    let p = (uv - centre) * aspect;
    let radius = length(p);
    let angle = atan2(p.y, p.x);
    let ripple = sin(radius * 32.0 - phase * 6.0 + 0.7 * sin(angle * 3.0 + phase * 2.0));
    let breath = 0.02 * sin(phase * 2.0) + 0.012 * ripple;
    let eddy = sin(angle * 3.0 + radius * 18.0 - phase * 4.0) * cos(phase * 3.0 + radius * 11.0);
    let curl = vec2<f32>(
        sin(p.y * 23.0 + phase * 3.0) + 0.5 * sin(p.x * 17.0 - phase * 5.0),
        cos(p.x * 21.0 - phase * 4.0) + 0.5 * cos(p.y * 15.0 + phase * 7.0)
    ) * vec2<f32>(0.004, 0.006);
    let envelope = smoothstep(0.05, 0.18, radius) * (1.0 - smoothstep(0.6, 0.9, radius));
    let drift = (p * breath + vec2<f32>(-p.y, p.x) * (0.055 * eddy) + curl) * envelope;
    let energy = 0.86 + 0.28 * smoothstep(-0.2, 0.8, ripple) + 0.10 * smoothstep(0.2, 0.9, eddy);
    return vec3<f32>(centre + (p + drift) / aspect, energy);
}

@fragment fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let reference = vec2<f32>(2000.0, 1122.0);
    let scale = max(u.rect.z / reference.x, u.rect.w / reference.y);
    let uv = (in.local - u.rect.zw * 0.5) / (reference * scale) + 0.5;
    var colour: vec3<f32>;
    if u.animation.y > 0.5 {
        let phase = u.animation.x * 6.2831853;
        let field = current(uv, phase);
        let grid = field.xy * vec2<f32>(300.0, 100.0);
        // The two sides of the disc stream in opposite directions. Integer cell
        // travel and whole wave frequencies stay continuous at the clock wrap.
        let row = floor(grid.y);
        let travel = clamp(43.0 - row, -16.0, 16.0) * 6.0;
        let shift = u.animation.x * travel
            + 1.6 * sin(phase * 5.0 + row * 0.11)
            + 0.8 * sin(phase * 7.0 - row * 0.23);
        let moving = grid - vec2<f32>(shift, 0.0);
        let within = fract(moving);
        let source = floor(moving) + vec2<f32>(shift, 0.0);
        let at = vec2<i32>(floor(source));
        // Read the supplied density and colour at each moving character's new
        // position. Interpolate on that same glyph footprint, keeping the disc
        // and void in place without double-exposed or rotating characters.
        colour = mix(ink(at, within, phase, field.z), ink(at + vec2<i32>(1, 0), within, phase, field.z), fract(source.x));
    } else {
        let grid = uv * vec2<f32>(300.0, 100.0);
        colour = ink(vec2<i32>(floor(grid)), fract(grid), 0.0, 1.0);
    }
    let alpha = clip_alpha(in.position.xy + u.viewport.zw, u.clip, u.fade);
    if u.animation.z > 0.5 {
        colour = srgb(colour);
    }
    return vec4<f32>(colour * alpha, alpha);
}
