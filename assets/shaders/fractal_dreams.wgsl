// Fractal Dreams - Mandelbrot psicodelico con colores hipnoticos
var uv = in.uv * 2.0 - 1.0;
uv.x *= u.aspect;

// Interactividad mejorada con el mouse para explorar el fractal
let mouse_influence = vec2<f32>(u.mouse_x, u.mouse_y) * 2.5;
var pos = uv * 1.2 + mouse_influence; // Movimiento mucho mas sensible al mouse

// Zoom dinamico basado en intensidad - aumentado para objeto mas grande
let zoom = 0.8 + sin(u.time * 0.2) * 0.3 * u.intensity; // Reducido base de 1.5 a 0.8 para mas zoom
pos *= zoom;

// Colores base con acento del tema
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);
var col = vec3<f32>(0.0);

// Iteracion del fractal Mandelbrot con colores psicodelicos
for (var i: f32 = 0.0; i < 100.0; i = i + 1.0) {
    // Formula del fractal con variacion temporal y seguimiento del mouse
    let mouse_center = vec2<f32>(u.mouse_x, u.mouse_y) * 0.8;
    let c = pos + mouse_center + vec2<f32>(sin(u.time * 0.1) * 0.3, cos(u.time * 0.15) * 0.2);
    pos = vec2<f32>(
        pos.x * pos.x - pos.y * pos.y + c.x,
        pos.x * pos.y * 2.0 + c.y
    );
    
    // Color basado en la velocidad de escape
    let escape = length(pos);
    if (escape > 2.0) {
        // Paleta psicodelica con acento
        let hue = i * 0.1 + u.time * 0.05;
        let sat = 0.8 + sin(i * 0.2) * 0.2;
        let val = 1.0 - (i / 100.0);
        
        // Convertir HSV a RGB simplificado
        let r = sin(hue) * sat * val;
        let g = sin(hue + 2.094) * sat * val;
        let b = sin(hue + 4.189) * sat * val;
        
        // Mezclar con acento del tema
        col = mix(vec3<f32>(r, g, b), accent, 0.3);
        break;
    }
}

// Efecto de pulso hipnotico
let pulse = sin(u.time * 3.0) * 0.5 + 0.5;
col *= (0.5 + pulse * 0.5) * u.intensity;

// Vineta mistica
let vign = 1.0 - smoothstep(0.8, 2.0, length(uv));
col *= vign;

return vec4<f32>(col, 1.0);
