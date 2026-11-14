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

@fragment fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let reference = vec2<f32>(2000.0, 1122.0);
    let scale = max(u.rect.z / reference.x, u.rect.w / reference.y);
    let uv = (in.local - u.rect.zw * 0.5) / (reference * scale) + 0.5;
    let grid = uv * vec2<f32>(300.0, 100.0);
    let cell = textureLoad(cells, vec2<i32>(floor(grid)), 0);
    let background = linear(vec3<f32>(8.0, 9.0, 11.0) / 255.0);
    var colour = background;
    if cell.a > 0u {
        let centre = (floor(grid) + 0.5) / vec2<f32>(300.0, 100.0);
        let p = (centre - vec2<f32>(0.52, 0.43)) * vec2<f32>(1.78, 1.0);
        let angle = atan2(p.y, p.x);
        let radius = length(p);
        let orbit = angle * 2.0 + radius * 9.0 - u.animation.x * 6.2831853 / 24.0;
        let wave = pow(0.5 + 0.5 * sin(orbit), 5.0);
        // Preserve the silhouette, void and palette. Only a small fraction of the
        // ink crossfades to the adjacent density glyph as the highlight passes.
        let motion = u.animation.y;
        let morph = smoothstep(0.55, 0.95, wave) * 0.22 * motion;
        let next = min(cell.a + 1u, 9u);
        let ink = mix(glyph(cell.a, fract(grid)), glyph(next, fract(grid)), morph);
        let brightness = mix(1.0, 0.94 + 0.18 * wave, motion);
        colour = mix(background, linear(vec3<f32>(cell.rgb) / 255.0) * brightness, ink);
    }
    let alpha = clip_alpha(in.position.xy + u.viewport.zw, u.clip, u.fade);
    if u.animation.z > 0.5 {
        colour = srgb(colour);
    }
    return vec4<f32>(colour * alpha, alpha);
}
