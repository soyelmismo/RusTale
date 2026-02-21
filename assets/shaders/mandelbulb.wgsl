fn map(p_in: vec3<f32>) -> vec4<f32> {
    // 1. Infinite Tunnel Tiling
    var p = p_in;
    p.z = p.z % 4.0 - 2.0; // Tile every 4 units on Z
    
    // 2. The "Safety Hole" (Guarantees no collisions)
    let tunnel_radius = 0.6;
    let d_tunnel = -(length(p.xy) - tunnel_radius);

    // 3. Fractal Folding Optimizado - Menos iteraciones y cálculos
    var scale = 1.0;
    var trap = 1.0;

    // Rotación sutil del espacio (optimizada)
    let rotation_angle = u.time * 0.05; // Más lento para menos cálculos
    let cos_a = cos(rotation_angle);
    let sin_a = sin(rotation_angle);
    let new_x = p.x * cos_a - p.y * sin_a;
    let new_y = p.x * sin_a + p.y * cos_a;
    p.x = new_x;
    p.y = new_y;

    for (var i = 0; i < 4; i++) { // Reducido de 6 a 4 iteraciones
        // Fold space simplificado (sin cálculos trigonométricos)
        let fold_offset = 1.1; // Constante en lugar de sin()
        p = abs(p) - fold_offset;
        let r2 = dot(p, p);
        
        // Sphere Fold estable
        if (r2 < 0.25) {
            p = p * 2.0;
            scale = scale * 2.0;
        } else if (r2 < 1.0) {
            p = p / r2;
            scale = scale / r2;
        }
        
        // Scale mínimo (sin variación temporal)
        let morph_factor = 1.05; // Constante pequeña
        p = p * morph_factor;
        scale = scale * morph_factor;
        
        // Offset mínimo constante
        p = p + vec3<f32>(0.05, 0.025, 0.035);
        
        trap = min(trap, r2);
    }
    
    let d_fractal = (length(p) - 1.2) / abs(scale);
    
    // Combine fractal with the safety tunnel (Subtraction)
    let final_d = max(d_fractal, d_tunnel);
    
    return vec4<f32>(final_d, trap, 0.0, 0.0);
}

fn raymarch(ro: vec3<f32>, rd: vec3<f32>) -> vec4<f32> {
    var t = 0.02;
    for (var i = 0; i < 48; i++) { // Reducido de 64 a 48 steps
        let res = map(ro + rd * t);
        if (res.x < 0.002) {
            return vec4<f32>(t, res.y, 0.0, 0.0);
        }
        t += res.x * 0.8; // Relaxed stepping para smoothness
        if (t > 12.0) { break; } // Reducido distancia de visión
    }
    return vec4<f32>(-1.0);
}

fn get_ao(p: vec3<f32>, n: vec3<f32>) -> f32 {
    var occ = 0.0;
    var sca = 1.0;
    for (var i = 0; i < 5; i++) {
        let h = 0.01 + 0.12 * f32(i) / 4.0;
        let d = map(p + h * n).x;
        occ += (h - d) * sca;
        sca *= 0.95;
    }
    return clamp(1.0 - 3.0 * occ, 0.0, 1.0);
}

fn get_normal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.001, 0.0);
    return normalize(vec3<f32>(
        map(p + e.xyy).x - map(p - e.xyy).x,
        map(p + e.yxy).x - map(p - e.yxy).x,
        map(p + e.yyx).x - map(p - e.yyx).x
    ));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = (in.uv * 2.0 - 1.0) * vec2<f32>(u.aspect, 1.0);
    
    // Slower path movement
    let time = u.time * 0.3; // Reducido de 0.8 a 0.3 para movimiento más lento
    
    // Camera con rotación lenta para ver diferentes ángulos
    let camera_radius = 0.3; // Radio de órbita de la cámara
    let rotation_speed = 0.15; // Velocidad de rotación lenta
    let camera_angle = time * rotation_speed;
    
    // Posición de cámara en órbita alrededor del centro del túnel
    let ro = vec3<f32>(
        sin(camera_angle) * camera_radius,  // Movimiento horizontal
        cos(camera_angle * 0.7) * 0.1,      // Movimiento vertical sutil
        time                                  // Movimiento forward constante
    );
    
    // Lookat con movimiento dinámico
    let lookat = vec3<f32>(
        sin(time * 0.1) * 0.05 + sin(camera_angle * 0.5) * 0.02,
        cos(time * 0.15) * 0.05 + cos(camera_angle * 0.3) * 0.02,
        time + 1.0
    );
    
    let cw = normalize(lookat - ro);
    let cu = normalize(cross(cw, vec3<f32>(0.0, 1.0, 0.0)));
    let cv = cross(cu, cw);
    let rd = normalize(uv.x * cu + uv.y * cv + 1.5);

    let res = raymarch(ro, rd);
    let base_color = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);

    if (res.x > 0.0) {
        // Orbit color based on the folding trap
        let neon = 0.5 + 0.5 * cos(vec3<f32>(0.0, 2.0, 4.0) + res.y * 10.0 + time);
        var col = mix(base_color, neon, 0.4);
        
        // Lighting
        let p = ro + rd * res.x;
        let fog = exp(-0.4 * res.x); // Thick fog hides pops
        
        // Intensity control
        var final_rgb = col * fog * u.intensity;
        
        // Add a soft glow from the tunnel center
        final_rgb += base_color * 0.1 / (res.x + 0.1); 
        
        return vec4<f32>(final_rgb, 1.0);
    }

    return vec4<f32>(0.0, 0.0, 0.0, 1.0); // Perfect black background
}
