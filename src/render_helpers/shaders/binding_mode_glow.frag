precision highp float;

#if defined(DEBUG_FLAGS)
uniform float niri_tint;
#endif

uniform float niri_alpha;
uniform float niri_scale;

uniform vec2 niri_size;
varying vec2 niri_v_coords;

uniform vec4 glow_color;
// How far the light reaches inward from the side edge, in logical pixels.
uniform float reach;
// Half the vertical extent of one arc, in logical pixels. The light is brightest at the
// vertical middle of the output and gone by this distance above and below it.
uniform float spread;

// A soft edge light on the left and right sides, brightest at the middle of each side.
//
// This says "a binding mode is active" without a word of text and without anything appearing
// where windows live. It is drawn over the whole output but is transparent almost everywhere:
// the two lit arcs hug the side edges, fade inward toward the centre of the screen the way a
// shadow does, and fade out towards the top and bottom, which is what gives each one its
// crescent shape.
//
// Both falloffs are smoothstep rather than linear. A linear ramp ends in a visible line where
// it reaches zero; smoothstep arrives flat, so the light has no border of its own.
void main() {
    vec2 px = niri_v_coords * niri_size;

    // Distance to the nearer of the two side edges. Only the sides are lit: a frame all the way
    // around reads as a border, which is a different and much louder thing.
    float from_side = min(px.x, niri_size.x - px.x);
    float across = 1.0 - smoothstep(0.0, max(reach, 1.0), from_side);
    // Squared, so the light hugs the very edge and gives up quickly instead of washing a
    // broad band across the side of the screen. Halving the opacity alone left it dim but
    // still wide, which reads as a panel rather than as a glow.
    across = across * across;

    // Distance from the vertical middle, so each arc is centred on its side.
    float from_middle = abs(px.y - niri_size.y * 0.5);
    float along = 1.0 - smoothstep(0.0, max(spread, 1.0), from_middle);

    vec4 color = glow_color * across * along;

    color = color * niri_alpha;

#if defined(DEBUG_FLAGS)
    if (niri_tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
