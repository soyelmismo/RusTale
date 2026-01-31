// Neon Oil Spill - Patrones fluidos recursivos
var uv = in.uv * 2.0 - 1.0;
uv.x *= u.aspect;

// Interactividad tactil con el mouse - genera ondas de choque al hacer clic
// mouse_x/y vienen normalizados (-1 a 1) desde el vertex shader
// u.intensity se usa como disparador de pulso momentaneo al hacer clic

// Onda de choque que se expande desde la posicion del mouse
let click_wave = sin(length(uv - vec2(u.mouse_x, -u.mouse_y)) * 20.0 - u.time * 10.0);

// Interaccion base del mouse con distorsion psicodelica
let mouse_interaction = vec2<f32>(u.mouse_x, -u.mouse_y) * (0.2 + (u.intensity * 0.05 * click_wave));
var pos = uv - mouse_interaction;

var final_col = vec3<f32>(0.0);
let t = u.time * 0.4;

// Bucle de distorsion fractal
for (var i: f32 = 1.0; i < 5.0; i = i + 1.0) {
    // Distorsion de coordenadas basada en seno/coseno
    //fract() crea repeticion de dominio
    pos = fract(pos * 1.2) - 0.5; 
    
    // Longitud para crear gradientes radiales
    let d = length(pos);
    
    // Desplazamiento dinamico (Movimiento fluido)
    // Se usa 'i' para desfasar cada capa y crear complejidad
    let shift = vec2<f32>(
        sin(pos.y * 3.0 + t * 0.5 + i),
        cos(pos.x * 3.0 - t * 0.8 + i * 2.0)
    );
    
    pos += shift * 0.4;
    
    // Generacion de color Paleta RGB basada en fases de tiempo
    // Usamos el indice 'i' para variar el color por capa
    let layer_col = vec3<f32>(
        sin(d * 4.0 + t + 0.0 + i) * 0.5 + 0.5,
        sin(d * 4.0 + t + 2.0 + i) * 0.5 + 0.5,
        sin(d * 4.0 + t + 4.0 + i) * 0.5 + 0.5
    );
    
    // Acumulacion inversa de la distancia para efecto "Brillo/Neon"
    // Evitamos division por cero sumando un epsilon (+0.2)
    // El abs() asegura que el brillo no sea negativo (aunque length es positivo)
    final_col += layer_col * (0.25 / max(0.1, abs(d) + 0.1));
}

// Normalizar un poco la intensidad despues de acumular 4 capas
final_col = final_col * 0.35;

// Anadir el color de acento del usuario (Naranja RusTale, etc.)
// Lo mezclamos sutilmente para tintar la psicodelia
let accent = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);
final_col = mix(final_col, accent, 0.15);

// Vineta para oscurecer los bordes y dar profundidad
let vign = 1.0 - smoothstep(0.5, 2.0, length(uv));
final_col *= vign;

// Salida final aplicando intensidad global (alpha manejado por el shader manager)
return vec4<f32>(final_col * u.intensity, 1.0);