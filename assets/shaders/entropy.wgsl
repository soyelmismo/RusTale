
// Funcion para crear una neurona individual
fn draw_neuron(pos: vec2<f32>, activation: f32, id: f32, accent: vec3<f32>) -> vec3<f32> {
    let dist = length(pos);
    let neuron_size = 0.08 + activation * 0.05; // Reducido para mas densidad
    let glow = 1.0 - smoothstep(0.0, neuron_size, dist);
    
    // Color base con acento
    let base_color = mix(
        vec3<f32>(0.2, 0.1, 0.4), // Purpura
        accent, // Acento del tema
        0.6
    );
    
    // Pulsacion hipnotica
    let pulse = sin(u.time * 3.0 + id) * 0.3 + 0.7;
    return base_color * glow * activation * pulse;
}

// Funcion helper para distancia a linea
fn distance_to_line_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ap = p - a;
    let ab = b - a;
    let t = clamp(dot(ap, ab) / dot(ab, ab), 0.0, 1.0);
    let closest = a + t * ab;
    return length(p - closest);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Neural Network - Red neuronal sinaptica con conexiones pulsantes
    var uv = in.uv * 2.0 - 1.0;
    uv.x *= u.aspect;

    // Grid de neuronas - 64 neuronas expandidas horizontalmente
    let grid_size_x = 16.0; // Expandido horizontalmente
    let grid_size_y = 8.0;  // Mantenido verticalmente
    var cell = floor(vec2<f32>(uv.x * grid_size_x, uv.y * grid_size_y));
    var cell_uv = fract(vec2<f32>(uv.x * grid_size_x, uv.y * grid_size_y)) - 0.5;

    // Color de acento para las neuronas
    let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);
    var col = vec3<f32>(0.02, 0.01, 0.05); // Fondo oscuro

    // Dibujar red de neuronas
    for (var x: f32 = 0.0; x < grid_size_x; x = x + 1.0) {
        for (var y: f32 = 0.0; y < grid_size_y; y = y + 1.0) {
            let neuron_id = x * grid_size_y + y;
            var neuron_pos = vec2<f32>(x, y) / vec2<f32>(grid_size_x, grid_size_y) * 2.0 - 1.0;
            neuron_pos.x *= 1.0 / u.aspect;
            
            // Activacion basada en tiempo y mouse
            let mouse_influence = 1.0 / (length(neuron_pos - vec2<f32>(u.mouse_x, u.mouse_y)) + 0.5);
            let wave_activation = sin(u.time * 2.0 + neuron_id * 0.5) * 0.5 + 0.5;
            let activation = (mouse_influence * 0.3 + wave_activation * 0.7) * u.intensity;
            
            // Dibujar neurona
            let local_uv = uv - neuron_pos;
            col += draw_neuron(local_uv, activation, neuron_id, accent);
            
            // Conexiones sinapticas
            if (x < grid_size_x - 1.0) {
                var next_pos = vec2<f32>(x + 1.0, y) / vec2<f32>(grid_size_x, grid_size_y) * 2.0 - 1.0;
                next_pos.x *= 1.0 / u.aspect;
                
                // Linea de conexion con efecto de flujo
                let connection_dist = distance_to_line_segment(uv, neuron_pos, next_pos);
                let flow = sin(u.time * 4.0 - length(uv - neuron_pos) * 5.0) * 0.5 + 0.5;
                let connection_glow = 0.02 / (connection_dist + 0.003); // Mas brillante y visible
                
                col += accent * connection_glow * flow * activation * 0.5; // Mas intenso
            }
            
            // Conexiones verticales
            if (y < grid_size_y - 1.0) {
                var next_pos = vec2<f32>(x, y + 1.0) / vec2<f32>(grid_size_x, grid_size_y) * 2.0 - 1.0;
                next_pos.x *= 1.0 / u.aspect;
                
                let connection_dist = distance_to_line_segment(uv, neuron_pos, next_pos);
                let flow = sin(u.time * 4.0 - length(uv - neuron_pos) * 5.0) * 0.5 + 0.5;
                let connection_glow = 0.02 / (connection_dist + 0.003); // Mas brillante y visible
                
                col += accent * connection_glow * flow * activation * 0.5; // Mas intenso
            }
        }
    }

    // Efecto de ondas cerebrales
    let brain_wave = sin(length(uv) * 10.0 - u.time * 3.0) * 0.5 + 0.5;
    col *= 0.8 + brain_wave * 0.2;

    // Post-proceso neuronal
    col = pow(col, vec3<f32>(1.2)); // Contraste aumentado

    return vec4<f32>(col, u.alpha);
}
