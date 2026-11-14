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

fn ink(at: vec2<i32>, within: vec2<f32>) -> vec3<f32> {
    let background = linear(vec3<f32>(8.0, 9.0, 11.0) / 255.0);
    if any(at < vec2<i32>(0)) || any(at >= vec2<i32>(300, 100)) {
        return background;
    }
    let cell = textureLoad(cells, at, 0);
    return mix(background, linear(vec3<f32>(cell.rgb) / 255.0), glyph(cell.a, within));
}

@fragment fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let reference = vec2<f32>(2000.0, 1122.0);
    let scale = max(u.rect.z / reference.x, u.rect.w / reference.y);
    let uv = (in.local - u.rect.zw * 0.5) / (reference * scale) + 0.5;
    let grid = uv * vec2<f32>(300.0, 100.0);
    var colour: vec3<f32>;
    if u.animation.y > 0.5 {
        // The two sides of the disc stream in opposite directions. Integer cell
        // travel per cycle makes the moving lattice seamless at the clock wrap.
        let travel = clamp(43.0 - floor(grid.y), -16.0, 16.0);
        let shift = u.animation.x * travel;
        let moving = grid - vec2<f32>(shift, 0.0);
        let within = fract(moving);
        let source = floor(moving) + vec2<f32>(shift, 0.0);
        let at = vec2<i32>(floor(source));
        // Read the supplied density and colour at each moving character's new
        // position. Interpolate on that same glyph footprint, keeping the disc
        // and void in place without double-exposed or rotating characters.
        colour = mix(ink(at, within), ink(at + vec2<i32>(1, 0), within), fract(source.x));
    } else {
        colour = ink(vec2<i32>(floor(grid)), fract(grid));
    }
    let alpha = clip_alpha(in.position.xy + u.viewport.zw, u.clip, u.fade);
    if u.animation.z > 0.5 {
        colour = srgb(colour);
    }
    return vec4<f32>(colour * alpha, alpha);
}
