// Liquid Consciousness - Conciencia liquida con ondas cerebrales
var uv = in.uv * 2.0 - 1.0;
uv.x *= u.aspect;

// Color de acento del tema
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);

// Sistema de ondas cerebrales multiples
var col = vec3<f32>(0.0);

// Ondas cerebrales con diferentes frecuencias (Alpha, Beta, Theta, Delta)
let brain_waves = array<vec3<f32>, 4>(
    vec3<f32>(0.5, 2.0, 4.0),   // Alpha: relajacion
    vec3<f32>(2.0, 5.0, 8.0),   // Beta: alerta
    vec3<f32>(0.2, 0.8, 1.5),   // Theta: meditacion
    vec3<f32>(0.1, 0.3, 0.7)    // Delta: sueño profundo
);

// Capas de conciencia liquida
for (var layer: f32 = 0.0; layer < 4.0; layer = layer + 1.0) {
    let wave_params = brain_waves[i32(layer)];
    
    // Distorsion liquida con influencia del mouse
    let mouse_distortion = vec2<f32>(u.mouse_x, u.mouse_y) * (0.1 + layer * 0.05);
    var liquid_uv = uv + mouse_distortion;
    
    // Ondas liquidas complejas
    let time_scale = u.time * (0.5 + layer * 0.3);
    let wave1 = sin(liquid_uv.x * wave_params.x + time_scale) * cos(liquid_uv.y * wave_params.y - time_scale * 0.7);
    let wave2 = sin(length(liquid_uv) * wave_params.z - time_scale * 1.3);
    let wave3 = cos(dot(liquid_uv, liquid_uv) * wave_params.x + time_scale * 0.9);
    
    // Combinacion de ondas para efecto liquido
    let liquid_pattern = (wave1 + wave2 + wave3) * 0.333;
    
    // Flujo de conciencia
    let flow_field = vec2<f32>(
        sin(liquid_uv.y * 3.0 + time_scale) * 0.1,
        cos(liquid_uv.x * 3.0 - time_scale) * 0.1
    );
    
    // Aplicar flujo a las coordenadas
    liquid_uv = liquid_uv + flow_field * layer * 0.2;
    
    // Patrones de conciencia fractales
    var fractal_pos = liquid_uv * (2.0 + layer);
    for (var i: f32 = 0.0; i < 3.0; i = i + 1.0) {
        fractal_pos = abs(fractal_pos) - 0.5;
        let rot_angle = time_scale * 0.1;
        fractal_pos = vec2<f32>(
            fractal_pos.x * cos(rot_angle) - fractal_pos.y * sin(rot_angle),
            fractal_pos.x * sin(rot_angle) + fractal_pos.y * cos(rot_angle)
        );
    }
    
    let fractal_intensity = 1.0 / (length(fractal_pos) + 0.5);
    
    // Paleta de colores de conciencia
    let consciousness_hue = u.time * 0.05 + layer * 1.57; // 90 grados entre capas
    let consciousness_color = vec3<f32>(
        sin(consciousness_hue) * 0.5 + 0.5,
        sin(consciousness_hue + 2.094) * 0.5 + 0.5,
        sin(consciousness_hue + 4.189) * 0.5 + 0.5
    );
    
    // Mezclar con acento del tema
    let layer_color = mix(consciousness_color, accent, 0.3 + layer * 0.1);
    
    // Combinar patrones liquidos con fractales
    let pattern_intensity = smoothstep(-0.8, 0.8, liquid_pattern) * fractal_intensity;
    
    // Efecto de respiracion de la conciencia
    let consciousness_breath = sin(u.time * (1.0 + layer * 0.5)) * 0.4 + 0.6;
    
    // Acumular capa
    col += layer_color * pattern_intensity * consciousness_breath * (1.0 - layer * 0.15);
}

// Burbujas de conciencia flotantes
let bubble_count = 15.0;
for (var i: f32 = 0.0; i < bubble_count; i = i + 1.0) {
    // Posicion de burbuja con movimiento organico
    let bubble_time = u.time * (0.3 + i * 0.1);
    let bubble_pos = vec2<f32>(
        sin(bubble_time + i * 1.5) * 0.7,
        cos(bubble_time * 0.8 + i * 2.0) * 0.5
    );
    
    let dist = length(uv - bubble_pos);
    let bubble_size = 0.05 + sin(u.time * 2.0 + i) * 0.02;
    let bubble_glow = 1.0 - smoothstep(0.0, bubble_size, dist);
    
    // Color de burbuja con iridiscencia
    let iridescence = sin(dist * 20.0 - u.time * 5.0 + i * 3.0) * 0.5 + 0.5;
    let bubble_color = mix(accent, vec3<f32>(1.0, 1.0, 1.0), iridescence);
    
    col += bubble_color * bubble_glow * 0.8;
}

// Efecto de pulso cerebral central
let brain_pulse = sin(u.time * 2.0) * 0.3 + 0.7;
let center_dist = length(uv);
let central_glow = 1.0 / (center_dist * 2.0 + 0.1);
col += accent * central_glow * brain_pulse * 0.5;

// Post-proceso de conciencia
col *= u.intensity; // Intensidad global
col = pow(col, vec3<f32>(1.1)); // Contraste sutil

// Efecto de "sueño" en los bordes
let dream_vign = 1.0 - smoothstep(0.6, 1.5, length(uv));
col *= dream_vign;

return vec4<f32>(col, 1.0);
