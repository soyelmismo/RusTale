// =========================================================
// Forest Shader - Siluetas Geométricas (Style: Firewatch/Limbo)
// =========================================================

// --- HERRAMIENTAS DE MATEMATICAS Y RUIDO ---

fn hash1(n: f32) -> f32 { return fract(sin(n) * 43758.5453123); }

fn smooth_time(t: f32, speed: f32) -> f32 {
    // Suaviza el tiempo para reducir flickering
    return sin(t * speed) * 0.5 + 0.5;
}

fn noise(x: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(hash1(i), hash1(i + 1.0), u);
}


// SDF de un Triangulo Isosceles (para los árboles)
fn sdTriangleIsosceles(p: vec2<f32>, q: vec2<f32>) -> f32 {
    var pp = p;
    pp.x = abs(pp.x);
    let a = pp - q * clamp(dot(pp, q) / dot(q, q), 0.0, 1.0);
    let b = pp - q * vec2<f32>(clamp(pp.x / q.x, 0.0, 1.0), 1.0);
    let k = sign(q.y);
    let d = min(dot(a, a), dot(b, b));
    let s = max(k * (pp.x * q.y - pp.y * q.x), k * (pp.y - q.y));
    return sqrt(d) * sign(s);
}

// --- DIBUJO DE OBJETOS ---

// Dibuja un solo árbol procedural
// uv: coordenadas locales centras en la base del árbol
// scale: altura del árbol
// seed: semilla aleatoria para que cada árbol sea unico
fn draw_tree(uv: vec2<f32>, scale: f32, seed: f32, wind: f32) -> f32 {
    var p = uv;
    // Viento: dobla la coordenada X cuanto más alto es Y
    p.x -= (p.y / scale) * (p.y / scale) * wind * 0.2;

    // 1. Tronco
    // Ancho variable segun altura
    let trunk_width = 0.02 * scale * (1.2 - p.y / scale);
    let trunk_dist = abs(p.x) - trunk_width;
    let trunk_mask = 1.0 - smoothstep(0.0, 0.005, trunk_dist);
    // Limitar altura del tronco
    let trunk_h_mask = step(0.0, p.y) * step(p.y, scale * 0.3);

    // 2. Copa (Ramas)
    // Apilamos 3 o 4 triangulos dentados
    var foliage: f32 = 0.0;
    
    // Altura base donde empiezan las ramas
    let start_y = scale * 0.15;
    let top_y = scale;
    
    // Si estamos debajo del follaje, devolver solo tronco
    if (p.y < start_y) {
        return trunk_mask * trunk_h_mask;
    }
    
    // Dibujamos capas de ramas
    let levels = 4.0;
    var current_foliage_dist = 100.0;
    
    for(var i: f32 = 0.0; i < levels; i = i + 1.0) {
        // Parametros por nivel de ramas
        let progress = i / levels;
        let y_pos = mix(start_y, top_y * 0.9, progress);
        let branch_scale = scale * (1.0 - progress * 0.8) * 0.5;
        let branch_w = branch_scale * 0.7; // Ancho del triángulo
        
        // Distancia local para este triángulo
        let tri_uv = vec2<f32>(p.x, p.y - y_pos);
        
        // Perturbación de los bordes (dentado)
        let jaggy = sin(p.y * 60.0 + seed) * 0.01 * scale;
        
        // SDF triángulo
        let tri = sdTriangleIsosceles(vec2<f32>(tri_uv.x + jaggy, tri_uv.y), vec2<f32>(branch_w, branch_scale));
        
        current_foliage_dist = min(current_foliage_dist, tri);
    }
    
    let foliage_mask = 1.0 - smoothstep(0.0, 0.01, current_foliage_dist);
    
    return max(trunk_mask * trunk_h_mask, foliage_mask);
}

// Genera una capa completa de terreno y árboles
// layer_idx: 0 es el fondo, números altos son primer plano
fn render_layer(uv: vec2<f32>, layer_idx: f32, layer_color: vec3<f32>) -> vec4<f32> {
    var col = vec4<f32>(0.0);
    
    // Ajustes de parálaje (horizontal y vertical)
    let scroll_x = u.mouse_x * (layer_idx + 1.0) * 0.2; 
    let scroll_y = u.mouse_y * (layer_idx + 1.0) * 0.1; // Menor movimiento vertical para naturalidad
    var p = uv;
    p.x += scroll_x;
    p.y += scroll_y;
    
    // --- TERRENO (SUELO) ---
    // Usamos ruido suave para crear colinas
    let terrain_freq = 0.5 + layer_idx * 0.1;
    // Frecuencia baja (forma general) + frecuencia alta (detalle)
    let ground_h = -0.5 + sin(p.x * terrain_freq + layer_idx) * 0.2 + noise(p.x * 2.0 + layer_idx) * 0.05;
    
    // Offset Y: capas lejanas más arriba visualmente
    let y_offset = (layer_idx * 0.15) - 0.4;
    let ground_y = ground_h + y_offset;
    
    // Máscara del suelo
    let in_ground = 1.0 - smoothstep(ground_y, ground_y + 0.01, p.y);
    
    if (in_ground > 0.0) {
        return vec4<f32>(layer_color, in_ground);
    }
    
    // --- ARBOLES ---
    // Dividimos el eje X en celdas (Grilla 1D) para posicionar árboles
    // Escala de la grilla depende de la profundidad de la capa
    let grid_size = 1.0 / (0.5 + layer_idx * 0.3); 
    let cell_id = floor(p.x / grid_size);
    let cell_x = fract(p.x / grid_size); // 0.0 a 1.0 dentro de la celda
    
    // Randoms por celda
    let h1 = hash1(cell_id + layer_idx * 33.0); // Random base
    let h2 = hash1(cell_id * 12.34);            // Posición
    let h3 = hash1(cell_id * 5.5 + u.time * 0.001); // Variedad más lenta
    
    var tree_acc = 0.0;
    
    // Probabilidad de que haya un árbol en esta celda
    if (h1 > 0.4) {
        // Posición X aleatoria dentro de la celda
        // Centro (0.5) +- jitter
        let tree_center_x = 0.5 + (h2 - 0.5) * 0.6;
        let dist_x = (cell_x - tree_center_x) * grid_size; // Distancia real en pantalla X
        
        // Calculamos la Y del suelo exactamente en el punto donde nace el árbol
        // para "plantarlo" correctamente
        let tree_world_x = (cell_id + tree_center_x) * grid_size;
        let ground_h_at_tree = -0.5 + sin(tree_world_x * terrain_freq + layer_idx) * 0.2 + noise(tree_world_x * 2.0 + layer_idx) * 0.05 + y_offset;
        
        let dist_y = p.y - ground_h_at_tree;
        
        // Propiedades del árbol
        let t_height = 0.6 + h2 * 0.6 + layer_idx * 0.1; // Altura variable
        let smooth_wind = smooth_time(u.time + cell_id, 0.8) * 2.0 - 1.0;
        let t_wind = smooth_wind * u.intensity * 1.5;
        
        // Coordenadas locales relativas a la base del árbol
        let t_uv = vec2<f32>(dist_x, dist_y);
        
        tree_acc = draw_tree(t_uv, t_height, h1 * 100.0, t_wind);
    }
    
    // Comprobar solapamiento de vecino (opcional, simplificado aquí cortando ancho de celda)
    // El 'tree_acc' es 0 o 1
    
    return vec4<f32>(layer_color, tree_acc);
}

// --- AVES (V SHAPES) ---

fn sd_bird_simple(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    // Alas en V
    p.x = abs(p.x);
    // Doblez simple hacia arriba
    return dot(p - vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0)); // muy simplificado
}

fn draw_birds_flock(uv_in: vec2<f32>) -> f32 {
    var alpha: f32 = 0.0;
    // Ajustar UV para el "plano de aves"
    // Hacemos que vuelen a la derecha
    var uv = uv_in;
    let fly_speed = 0.05; // Reducido para menos flickering
    uv.x -= u.time * fly_speed;
    
    // Añadir interacción con mouse vertical para las aves
    uv.y += u.mouse_y * 0.3;
    
    // Bucle para dibujar 3 aves diferentes con distintos offsets
    for (var i: i32 = 0; i < 3; i++) {
        let fi = f32(i);
        // Random pseudo offsets
        let offset = vec2<f32>(
            hash1(fi * 10.0),
            hash1(fi * 20.0) * 0.5 + 0.3 // Altura en el cielo
        );
        
        // Coordenada wrappeada (repeticion infinita en X)
        // Usamos un bloque grande de espacio
        let loop_space = vec2<f32>(3.0, 1.0); 
        
        // Posicion actual de este pájaro especifico en el mundo infinito
        // Offset Y le damos movimiento sinusoidal suave
        var pos = offset;
        let smooth_bob = smooth_time(u.time + fi * 0.3, 0.2) * 2.0 - 1.0;
        pos.y += smooth_bob * 0.03; // Más lento y sutil
        
        // Espacio local del pájaro con wrap
        // (uv.x + offset) % loop_width
        let loop_width = 2.5;
        let wrapped_x = uv.x + offset.x;
        let lx = fract(wrapped_x / loop_width) * loop_width - loop_width * 0.5; 
        let ly = uv.y - pos.y;
        
        var b_uv = vec2<f32>(lx, ly);
        
        // Animacion aleteo
        let wing_speed = 6.0 + fi * 1.0; // Reducido para menos flickering
        let smooth_flap = smooth_time(u.time + fi * 0.5, wing_speed) * 2.0 - 1.0; 
        
        // Escalarlo pequeño
        b_uv *= 25.0; // Zoom in
        
        // Deformar Y basado en X y flap para hacer el " aleteo"
        // Si flap es alto, las alas suben (V cerrada), si es bajo, bajan (V plana)
        let wing_y = pow(abs(b_uv.x), 1.5) * smooth_flap * 1.5;
        b_uv.y += wing_y;
        
        // Forma de V espesor
        let thickness = 0.2 - abs(b_uv.x) * 0.1;
        let d = length(vec2<f32>(b_uv.x, max(0.0, abs(b_uv.y) - thickness))) - 0.05;
        
        // Máscara
        let mask = 1.0 - smoothstep(0.0, 0.1, d);
        // Cortar lejanos en X para que no se vean infinitos bugs
        let clip = 1.0 - smoothstep(0.8, 1.0, abs(b_uv.x));
        
        alpha += mask * clip;
    }
    
    return clamp(alpha, 0.0, 1.0);
}


// --- MAIN SHADER ---

var uv = in.uv * 2.0 - 1.0;
uv.x *= u.aspect;

// Ajuste para dibujar de abajo a arriba
// uv.y += 0.0;

// 1. CIELO
// Gradiente amanecer/atardecer
// Tinte basado en Accent del usuario (Tema)
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);
// Cielo base: Morado oscuro arriba, Naranja/Rojo abajo
let sky_top = vec3<f32>(0.05, 0.02, 0.1) + accent * 0.1;
let sky_bot = mix(vec3<f32>(0.4, 0.2, 0.1), accent, 0.4); // Más influencia del tema abajo
var col = mix(sky_bot, sky_top, uv.y * 0.5 + 0.5);

// Sol/Luna detras
let sun_p = vec2<f32>(0.3, 0.3);
let sun_d = length(uv - sun_p);
// Disco solar brillante
let sun_disk = 1.0 - smoothstep(0.1, 0.11, sun_d);
// Glow solar
let sun_glow = 1.0 - smoothstep(0.1, 0.6, sun_d);
col += vec3<f32>(1.0, 0.9, 0.6) * sun_disk * 0.5;
col += accent * sun_glow * 0.4;

// 2. CAPAS DE BOSQUE
// Dibujamos de atrás hacia adelante (algoritmo del pintor)
let num_layers = 5.0;

for (var i: f32 = 0.0; i < num_layers; i = i + 1.0) {
    let t = i / (num_layers - 1.0); // 0.0 al fondo, 1.0 al frente
    
    // Color de la capa
    // Fondo: Color cielo desaturado y claro (Niebla)
    // Frente: Muy oscuro (Contraluz) y con toque de Accent
    var layer_color = mix(sky_bot * 0.5, vec3<f32>(0.02, 0.02, 0.02), t * t);
    
    // Añadimos un poco de 'fog' del acento a las capas intermedias
    if (t < 0.8 && t > 0.2) {
        layer_color += accent * 0.05;
    }
    
    let layer_data = render_layer(uv, i, layer_color);
    // data.rgb = color, data.a = alpha (máscara de árboles/suelo)
    
    // Blend normal (Alpha over)
    col = mix(col, layer_data.rgb, layer_data.a);
    
    // Niebla volumétrica ligera entre capas
    if (i < num_layers - 1.0) {
        // Cuanto más abajo en Y, más niebla acumulada
        let ground_fog = smoothstep(-0.5, -1.0, uv.y); 
        col = mix(col, layer_color, ground_fog * 0.15);
    }
}

// 3. AVES
let birds = draw_birds_flock(uv);
col = mix(col, vec3<f32>(0.0, 0.0, 0.0), birds);

// 4. POST PROCESO Y FADE
// Viñeta
col *= 1.1 - length(uv * 0.6);

// IMPORTANTE: Intensidad del sistema (Fade in/out controlado por tu Rustale)
col *= u.intensity; 

// Gamma correction fake
col = pow(col, vec3<f32>(1.0 / 1.2));

return vec4<f32>(col, 1.0);