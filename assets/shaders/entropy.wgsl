// =========================================================
// ENTROPY FRACTAL - PSYCHEDELIC MANDELBROT/JULIA HYBRID
// =========================================================

// --- MAIN SHADER ---
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv = in.uv * 2.0 - 1.0;
    uv.x *= u.aspect;

    // 1. INYECCIÓN DE ENTROPÍA (KERNEL SEEDS)
    // Convertimos los números aleatorios de Rust (alpha/transition) en coordenadas iniciales del caos.
    // Esto asegura que el patrón fractal base sea ÚNICO cada vez que abres el programa.
    let seed_chaos_x = sin(u.alpha * 0.013); // Frecuencias arbitrarias para mezclar bits
    let seed_chaos_y = cos(u.transition * 0.017);
    
    // Zoom base variable + Zoom respiratorio más rápido
    let zoom_seed = 0.8 + (sin(u.transition * 0.02) * 0.3); 
    var pos = uv * (zoom_seed - (sin(u.time * 0.3) * 0.2));

    // 2. ENTROPÍA DEL MOUSE (Estela Generadora de Llaves)
    // En lugar de solo mover la camara, el mouse inyecta una perturbación en el Espacio Complejo.
    // Esto "derrite" el fractal donde pasas el mouse.
    let m_pos = vec2<f32>(u.mouse_x, u.mouse_y);
    let dist_m = length(uv - vec2<f32>(m_pos.x, -m_pos.y));
    
    // La "estela" es una deformación angular basada en la proximidad (más sutil)
    let mouse_warp = 0.8 / (dist_m * 6.0 + 0.5); 
    pos += vec2<f32>(sin(m_pos.x * 2.0 + u.time * 0.5), cos(m_pos.y * 2.0 + u.time * 0.5)) * mouse_warp * 0.1;

    // Colores base con acento del tema
    let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);
    var col = vec3<f32>(0.0);

    // Variables acumulativas para el fractal
    var z = pos;
    // C es la constante mágica. Al mezclar el Seed del Kernel con el Mouse,
    // creamos un conjunto de Julia dinámico que nunca se repite.
    // Añadimos movimiento autónomo con múltiples frecuencias (mouse más sutil)
    let c = vec2<f32>(
        -0.7269 + seed_chaos_x * 0.1 + (u.mouse_x * 0.1) + sin(u.time * 0.4) * 0.05, 
        0.1889 + seed_chaos_y * 0.1 - (u.mouse_y * 0.1) + cos(u.time * 0.3) * 0.05
    );

    // Iteración del Fractal (Híbrido Mandelbrot/Julia para máxima psicodelia)
    var iter_val = 0.0;
    
    for (var i: f32 = 0.0; i < 64.0; i = i + 1.0) {
        // Z = Z^2 + C (Fórmula sagrada del fractal)
        // Aplicamos una rotación temporal más rápida para que esté "vivo"
        let t_rot = u.time * 0.5 + (seed_chaos_x * 10.0);
        let z_new = vec2<f32>(
            (z.x * z.x - z.y * z.y) + c.x + sin(t_rot)*0.08,
            (2.0 * z.x * z.y) + c.y + cos(t_rot)*0.08
        );
        z = z_new;

        // Velocidad de escape
        if (length(z) > 4.0) {
            // Suavizado de bandas para que se vea líquido y no a bloques
            // Normalizamos entre 0 y 1
            iter_val = i / 64.0; 
            break;
        }
    }

    // 3. COLORACIÓN PSICOTRÓPICA
    if (iter_val > 0.0) {
        // Paleta de ciclos basada en el tiempo y el acento (más rápida)
        // El 'iter_val' controla qué tan profundo entramos en el infinito
        
        let hue_speed = u.time * 0.8;
        let spectrum = iter_val * 6.0 + dist_m * 0.3; // La estela del mouse cambia el color localmente (más sutil)

        let r = 0.5 + 0.5 * cos(3.0 + spectrum * 3.0 + hue_speed + u.accent_r);
        let g = 0.5 + 0.5 * cos(3.0 + spectrum * 3.0 + hue_speed + 2.0 + u.accent_g);
        let b = 0.5 + 0.5 * cos(3.0 + spectrum * 3.0 + hue_speed + 4.0 + u.accent_b);

        let fractal_col = vec3<f32>(r, g, b);
        
        // Mezclar con el acento del usuario para mantener la identidad visual del launcher (más dinámico)
        col = mix(fractal_col, accent * (2.0 + sin(u.time * 2.0) * 0.8), 0.4);
        
        // Añadir brillo ("Bloom") basado en iteraciones cercanas (más intenso)
        col *= (1.2 + iter_val * 3.0);
    } 
    else {
        // Interior del fractal (Vacio profundo)
        // Lo teñimos ligeramente del acento muy oscuro
        col = accent * 0.05;
    }

    // Efecto visual extra: Ruido granulado (Film Grain) más dinámico usando el seed del Kernel
    // Ayuda a la sensación de "entropía cruda"
    let grain = fract(sin(dot(uv * (u.time * 0.3), vec2<f32>(12.9898, 78.233))) * 43758.5453);
    col += grain * 0.05;

    // Vineta mistica
    let vign = 1.0 - smoothstep(0.5, 2.0, length(uv));
    col *= vign;

    // Aplicar intensidad global (Fade in/out al cargar)
    return vec4<f32>(col * u.intensity, 1.0);
}