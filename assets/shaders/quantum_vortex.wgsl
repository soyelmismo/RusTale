// Quantum Vortex - Vortice cuantico con particulas de energia
var uv = in.uv * 2.0 - 1.0;
uv.x *= u.aspect;

// Centro del vortice con influencia del mouse
let center = vec2<f32>(u.mouse_x * 0.5, u.mouse_y * 0.5);
var pos = uv - center;

// Sistema de coordenadas polares para el vortice
let angle = atan(pos.y / pos.x);
let radius = length(pos);

// Distorsion espiral hipnotica
let spiral_twist = angle + radius * 5.0 - u.time * 2.0;
let spiral_distortion_f32 = sin(spiral_twist) * 0.1;

// Multiples capas de vortice
var col = vec3<f32>(0.0);
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);

for (var layer: f32 = 0.0; layer < 5.0; layer = layer + 1.0) {
    // Cada capa rota a diferente velocidad
    let layer_rotation = u.time * (1.0 + layer * 0.3);
    let rotated_angle = angle + layer_rotation;
    
    // Radio modificado para cada capa
    let layer_radius_f32 = radius * (1.0 + layer * 0.2) + spiral_distortion_f32;
    
    // Crear anillos de energia
    let ring_frequency_f32 = 8.0 + layer * 2.0;
    let ring_calc = layer_radius_f32 * ring_frequency_f32 - u.time * 4.0;
    let ring_pattern_f32 = sin(ring_calc);
    let ring_intensity = smoothstep(0.0, 1.0, ring_pattern_f32);
    
    // Color de la capa con influencia del acento
    let layer_color = mix(
        vec3<f32>(0.1, 0.05, 0.2), // Azul profundo
        accent, // Color de acento
        layer / 5.0
    );
    
    // Añadir energia cuantica
    let quantum_energy = sin(rotated_angle * 8.0 + u.time * 6.0) * 0.5 + 0.5;
    let final_intensity = ring_intensity * quantum_energy * (1.0 - layer * 0.15);
    
    // Acumular color con modo aditivo
    col += layer_color * final_intensity * u.intensity;
}

// Particulas de energia cuantica
let particle_count = 50.0;
for (var i: f32 = 0.0; i < particle_count; i = i + 1.0) {
    // Posicion de particula con movimiento orbital
    let particle_angle = (i / particle_count) * 6.28318 + u.time * (2.0 + i * 0.1);
    let particle_radius = 0.3 + sin(u.time + i) * 0.2;
    let particle_pos = vec2<f32>(
        cos(particle_angle) * particle_radius,
        sin(particle_angle) * particle_radius
    );
    
    // Distancia a la particula
    let dist = length(uv - particle_pos - center);
    let particle_glow = 0.02 / (dist + 0.01);
    
    // Color de particula pulsante
    let particle_pulse = sin(u.time * 10.0 + i * 2.0) * 0.5 + 0.5;
    col += accent * particle_glow * particle_pulse * 0.5;
}

// Efecto de agujero negro en el centro
let black_hole = smoothstep(0.1, 0.0, radius);
col *= (1.0 - black_hole);

// Post-proceso cosmico
col = pow(col, vec3<f32>(0.8)); // Gamma correction
col *= 1.0 - smoothstep(0.5, 1.5, length(uv)); // Vineta

return vec4<f32>(col, 1.0);
