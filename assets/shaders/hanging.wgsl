// =========================================================
// Hanging Shader
// =========================================================

// --- UTILS (RUIDO Y MATEMATICAS) ---

fn hash(n: f32) -> f32 {
    return fract(sin(n) * 43758.5453);
}

fn noise_(x: vec2<f32>) -> f32 {
    let p = floor(x);
    let f = fract(x);
    let k = f * f * (3.0 - 2.0 * f);
    
    let n = p.x + p.y * 57.0;
    
    return mix(mix(hash(n + 0.0), hash(n + 1.0), k.x),
               mix(hash(n + 57.0), hash(n + 58.0), k.x), k.y);
}

fn sdBird(p_in: vec2<f32>, t: f32) -> f32 {
    var p = p_in;
    p.y += sin(p.x * 10.0 - t * 15.0) * 0.05 * smoothstep(0.0, 0.2, abs(p.x));
    p.x = abs(p.x);
    let wing = dot(p, vec2<f32>(0.5, 0.2)) - 0.02; 
    let body = length(p) - 0.08;
    return max(wing, -body);
}

// --- LOGICA DE RENDERIZADO DEL BOSQUE ---

fn tree_layer(uv: vec2<f32>, layer_idx: f32, color_tint: vec3<f32>) -> vec4<f32> {
    let parallax = (u.mouse_x * 0.2) * (layer_idx + 1.0);
    var p = uv;
    p.x += parallax + layer_idx * 13.3; 
    
    p.x *= (1.5 + layer_idx * 0.5); 
    
    let id = floor(p.x);
    var local_x = fract(p.x) - 0.5;
    
    let h_rnd = hash(id * 12.3 + layer_idx);
    let height = 0.5 + h_rnd * 0.8;
    let width = 0.2 + h_rnd * 0.1;
    
    let wind_strength = 0.05 * (layer_idx * 0.5 + 0.5) * u.intensity; 
    let wind_freq = 2.0 + layer_idx;
    let sway = sin(u.time * wind_freq + id + p.y * 3.0) * wind_strength * smoothstep(0.0, 1.0, p.y + 1.0);
    local_x -= sway;
    
    let local_y = p.y + 1.0; 
    let branch_noise = noise_(vec2<f32>(local_x * 10.0, p.y * 20.0 + id));
    
    let slope = local_y * width; 
    let edge_w = 0.02; 
    
    let in_tree = smoothstep(slope + branch_noise * 0.05, slope - edge_w, abs(local_x));
    let vertical_limit = smoothstep(height, height - 0.1, local_y);
    
    var alpha = in_tree * vertical_limit;
    
    let gradient = mix(color_tint * 0.6, color_tint * 1.3, local_y / height);
    let shadow = smoothstep(-0.2, 0.3, local_x);
    let final_col = mix(gradient, gradient * 0.7, shadow);
    
    return vec4<f32>(final_col, alpha);
}

fn render_birds(uv_in: vec2<f32>, col_bg: vec3<f32>) -> vec3<f32> {
    var col = col_bg;
    var uv = uv_in;
    
    uv.x -= u.time * 0.3; 
    uv.y -= sin(u.time * 0.5) * 0.1; 
    
    let bird_grid = vec2<f32>(2.0, 1.5);
    let id = floor(uv / bird_grid);
    var p = fract(uv / bird_grid) * bird_grid - (bird_grid * 0.5);
    
    let rnd = hash(id.x * 7.1 + id.y * 3.3);
    
    if (rnd > 0.75) {
        p.x += (rnd - 0.5);
        p.y += (hash(rnd) - 0.5);
        
        let bird_dist = sdBird(p * 8.0, u.time * 12.0 + rnd * 10.0);
        let bird_alpha = 1.0 - smoothstep(0.005, 0.02, bird_dist);
        
        let bird_col = vec3<f32>(0.05, 0.02, 0.05) + vec3<f32>(u.accent_r, u.accent_g, u.accent_b) * 0.1;
        col = mix(col, bird_col, bird_alpha);
    }
    
    return col;
}

// --- MAIN SHADER ---

var uv = in.uv * 2.0 - 1.0;
uv.x *= u.aspect;

// Cielo
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);
var col = mix(vec3<f32>(0.05, 0.1, 0.2), vec3<f32>(0.6, 0.3, 0.2) * 0.5 + accent * 0.3, uv.y + 0.5);

// Sol
let sun_pos = vec2<f32>(0.3, 0.4);
let sun_dist = length(uv - sun_pos);
let sun = smoothstep(0.3, 0.05, sun_dist);
col += accent * sun * 0.3;

// Aves
col = render_birds(uv, col);

// Capas Arboles
let num_layers = 4.0;

for (var i: f32 = 0.0; i < num_layers; i = i + 1.0) {
    let layer_norm = i / (num_layers - 1.0);
    
    let base_tree = vec3<f32>(0.02, 0.08, 0.05); 
    
    // --- CORRECCIÓN AQUÍ: usamos 'var' porque reasignamos abajo ---
    var layer_col = mix(vec3<f32>(0.1, 0.15, 0.2) + accent * 0.1, base_tree, layer_norm * layer_norm);
    
    if (i == num_layers - 1.0) {
        layer_col = mix(layer_col, accent * 0.2, 0.2);
    }

    let layer_res = tree_layer(uv, i, layer_col);
    
    col = mix(col, layer_res.rgb, layer_res.a);
    
    if (i < num_layers - 1.0) {
        let fog = smoothstep(-0.5, -1.0, uv.y) * 0.2;
        col = mix(col, vec3<f32>(0.05, 0.1, 0.2), fog);
    }
}

// Post-proceso
let vig = 1.0 - length(uv * 0.5);
col *= smoothstep(0.0, 1.0, vig);
col *= u.intensity;

return vec4<f32>(col, 1.0);