// Kaleidoscope Dreams - Caleidoscopio psicodelico infinito
var uv = in.uv * 2.0 - 1.0;
uv.x *= u.aspect;

// Transformacion a coordenadas polares
let angle = atan(uv.y / uv.x);
let radius = length(uv);

// Color de acento del tema
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);

// Sistema caleidoscopico con multiples simetrias
let symmetries = 6.0; // 6 simetrias para hexagono
var col = vec3<f32>(0.0);

// Crear patrones caleidoscopicos
for (var layer: f32 = 0.0; layer < 3.0; layer = layer + 1.0) {
    // Cada capa con diferente rotacion y escala
    let layer_rotation = u.time * (1.0 + layer * 0.5);
    let layer_scale = 1.0 + layer * 0.3;
    
    // Coordenadas transformadas
    let transformed_angle = (angle + layer_rotation) * symmetries;
    let transformed_radius = radius * layer_scale;
    
    // Convertir de vuelta a cartesianas
    var pattern_uv = vec2<f32>(
        cos(transformed_angle) * transformed_radius,
        sin(transformed_angle) * transformed_radius
    );
    
    // Patrones geometricos complejos
    let pattern1 = sin(pattern_uv.x * 8.0 + u.time * 2.0) * cos(pattern_uv.y * 6.0 - u.time * 1.5);
    let pattern2 = sin(length(pattern_uv) * 10.0 - u.time * 3.0);
    let pattern3 = cos(dot(pattern_uv, pattern_uv) * 5.0 + u.time * 2.5);
    
    // Combinacion de patrones
    let combined_pattern = (pattern1 + pattern2 + pattern3) * 0.333;
    
    // Paleta de colores psicodelicos para esta capa
    let hue_shift = layer * 2.094; // 120 grados entre capas
    let time_hue = u.time * 0.1 + hue_shift;
    
    // Generar colores RGB desde HSV simplificado
    let r = sin(time_hue) * 0.5 + 0.5;
    let g = sin(time_hue + 2.094) * 0.5 + 0.5;
    let b = sin(time_hue + 4.189) * 0.5 + 0.5;
    
    let layer_color = vec3<f32>(r, g, b);
    
    // Mezclar con acento del tema
    let final_color = mix(layer_color, accent, 0.4);
    
    // Intensidad basada en el patron
    let intensity = smoothstep(-0.5, 0.5, combined_pattern);
    
    // Efecto de respiracion hipnotica
    let breath = sin(u.time * 2.0 + layer) * 0.3 + 0.7;
    
    // Acumular color con transparencia
    col += final_color * intensity * breath * (1.0 - layer * 0.2);
}

// Efecto de portal central
let portal_radius = 0.2 + sin(u.time * 3.0) * 0.05;
let portal_intensity = 1.0 - smoothstep(portal_radius * 0.5, portal_radius, radius);
let portal_color = mix(accent, vec3<f32>(1.0, 1.0, 1.0), portal_intensity);
col += portal_color * portal_intensity * 2.0;

// Particulas flotantes caleidoscopicas
let particle_count = 20.0;
for (var i: f32 = 0.0; i < particle_count; i = i + 1.0) {
    // Posicion de particula con movimiento espiral
    let particle_angle = (i / particle_count) * 6.28318 + u.time * (0.5 + i * 0.1);
    let particle_radius = 0.6 + sin(u.time + i * 0.5) * 0.3;
    let particle_pos = vec2<f32>(
        cos(particle_angle) * particle_radius,
        sin(particle_angle) * particle_radius
    );
    
    // Aplicar simetria caleidoscopica a la particula
    let sym_angle1 = particle_angle + 3.14159;
    let sym_angle2 = particle_angle + 6.28318;
    
    for (var j: f32 = 0.0; j < 3.0; j = j + 1.0) {
        let sym_angle = select(particle_angle, select(sym_angle1, sym_angle2, j >= 2.0), j >= 1.0);
        let sym_pos = vec2<f32>(
            cos(sym_angle) * particle_radius,
            sin(sym_angle) * particle_radius
        );
        
        let dist = length(uv - sym_pos);
        let particle_glow = 0.02 / (dist + 0.01);
        
        // Color de particula con arcoiris
        let particle_hue = u.time * 0.2 + i * 0.3 + j * 0.2;
        let pr = sin(particle_hue) * 0.5 + 0.5;
        let pg = sin(particle_hue + 2.094) * 0.5 + 0.5;
        let pb = sin(particle_hue + 4.189) * 0.5 + 0.5;
        
        let particle_pulse = sin(u.time * 8.0 + i * 2.0) * 0.5 + 0.5;
        col += vec3<f32>(pr, pg, pb) * particle_glow * particle_pulse * 0.5;
    }
}

// Post-proceso mistico
col *= u.intensity; // Aplicar intensidad global
col = pow(col, vec3<f32>(0.9)); // Gamma correction sutil

// Vineta cosmica
let vign = 1.0 - smoothstep(0.7, 1.8, length(uv));
col *= vign;

return vec4<f32>(col, 1.0);
