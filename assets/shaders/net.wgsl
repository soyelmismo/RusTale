var uv = in.uv * 2.0 - 1.0; // Centrar -1..1
uv.x *= u.aspect;

// Parallax mouse simple
let m = vec2<f32>(u.mouse_x, -u.mouse_y) * 0.5;
uv += m * 0.2;

var p = vec3<f32>(uv * 2.5, -u.time * 2.0); 
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);
var accum = 0.0;

// Rotacion camara CPU simulada
let t_rot = u.time * 0.2;
let s = sin(t_rot); let c = cos(t_rot);
let px = p.x * c - p.y * s;
p.y = p.x * s + p.y * c;
p.x = px;

for (var i = 0.0; i < 4.0; i += 1.0) {
    p.z = fract(p.z) - 0.5;
    p = abs(p) - 0.3;
    
    let sub_s = sin(u.time * 0.3 + i);
    let sub_c = cos(u.time * 0.3 + i);
    let pnz = p.z * sub_c - p.x * sub_s;
    p.x = p.z * sub_s + p.x * sub_c;
    p.z = pnz;

    let dist = max(max(abs(p.x), abs(p.y)), abs(p.z));
    let edge = abs(dist - 0.25);
    let glow = 0.012 / (edge + 0.005);
    let fade = 1.0 - (i / 4.0);
    accum += glow * fade;
}

var col = accent * accum * min(u.intensity, 1.5); 
// Vineta
col *= 1.0 - dot(uv, uv) * 0.4;

return vec4<f32>(col, 1.0);
