// Mochi's look — one drawing kit shared by the live island (engine.ts), the
// launch greeting and the drop sequence, so the three can never drift apart.
//
// The character: a plum helmet, a cream face plate with two ink eyes and pink
// cheeks, teal headphones, a striped scarf, a graduation cap and a gold crown
// whose five medallions ("dots") double as a live status strip for Claude Code:
//
//     0 honeycomb → searching        3 brain      → thinking
//     1 bolt      → working          4 trend/✓    → finished · rate limit
//     2 centre    → needs you: ! (approval)  ? (question)  × (error)
//
// Everything is in units of R (half the helmet's visual width ≈ 1.14 R). All
// helpers draw around the origin; the caller has already translated / rotated /
// squashed the context.

import { lerp } from "../core/anim";

export type RGB = readonly [number, number, number]; // 0…1

export const PAL = {
  helmetTop: "#A74B88",
  helmetBottom: "#7E2C63",
  helmetRim: "rgba(60,14,44,0.55)",
  plateTop: "#FFFAEF",
  plateBottom: "#F4EAD6",
  ink: "#1A1412",
  cheek: "rgba(246,168,170,",
  phoneDark: "#3D7893",
  phoneLight: "#86BACB",
  goldTop: "#FAD35E",
  goldBottom: "#EDB33C",
  goldRim: "#C98F27",
  medalIdle: "#F2C14E",
  cream: "#FFF6E0",
  capTop: "#403B50",
  capEdge: "#675F82",
  capUnder: "#1F1C28",
  tassel: "#9A3978",
  scarf: "#F6C445",
  scarfShade: "#E4A92D",
  stripeA: "#C0407E",
  stripeB: "#6F6B88",
} as const;

export const rgba = (c: RGB, a = 1) =>
  `rgba(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)},${a})`;

const mix = (a: RGB, b: RGB, t: number): RGB => [
  lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t),
];

export function roundRect(
  x: CanvasRenderingContext2D, X: number, Y: number, W: number, H: number, R: number,
) {
  const r = Math.max(0, Math.min(R, W / 2, H / 2));
  x.beginPath();
  x.moveTo(X + r, Y);
  x.arcTo(X + W, Y, X + W, Y + H, r);
  x.arcTo(X + W, Y + H, X, Y + H, r);
  x.arcTo(X, Y + H, X, Y, r);
  x.arcTo(X, Y, X + W, Y, r);
  x.closePath();
}

// ── What the five medallions show ─────────────────────────────────────────────

export type MedalGlyph = "bang" | "question" | "cross" | "check" | null;

export interface MedalSpec {
  /** Which medallion lights up, 0…4, or -1 for none. */
  slot: number;
  glyph: MedalGlyph;
  /** Idle glow of the other four (0…1). */
  rest: number;
}

/** One table to tweak if the mapping above should change. */
export const MEDAL_FOR_STATE: Record<string, MedalSpec> = {
  idle: { slot: -1, glyph: null, rest: 0 },
  sleeping: { slot: -1, glyph: null, rest: 0 },
  dizzy: { slot: -1, glyph: null, rest: 0 },
  searching: { slot: 0, glyph: null, rest: 0.1 },
  working: { slot: 1, glyph: null, rest: 0.1 },
  approval: { slot: 2, glyph: "bang", rest: 0.1 },
  question: { slot: 2, glyph: "question", rest: 0.1 },
  error: { slot: 2, glyph: "cross", rest: 0.1 },
  thinking: { slot: 3, glyph: null, rest: 0.1 },
  finished: { slot: 4, glyph: "check", rest: 0.28 },
  ratelimit: { slot: 4, glyph: null, rest: 0.1 },
};

/** Where the crown's five medallions sit (x, y, radius), in R. */
const MEDALS: readonly (readonly [number, number, number])[] = [
  [-0.66, -0.55, 0.155],
  [-0.34, -0.6, 0.155],
  [0, -0.64, 0.2],
  [0.34, -0.6, 0.155],
  [0.66, -0.55, 0.155],
];

// ── Helmet + face ─────────────────────────────────────────────────────────────

export function fillHelmet(
  x: CanvasRenderingContext2D, body: Path2D, R: number, rx: number, ry: number,
  solid: RGB | null,
) {
  if (solid) {
    const g = x.createLinearGradient(rx * 0.7, -ry * 0.85, -rx * 0.8, ry * 0.9);
    g.addColorStop(0, rgba(mix(solid, [1, 1, 1], 0.18)));
    g.addColorStop(1, rgba(mix(solid, [0, 0, 0], 0.12)));
    x.fillStyle = g;
  } else {
    const g = x.createLinearGradient(-rx * 0.5, -ry, rx * 0.6, ry);
    g.addColorStop(0, PAL.helmetTop);
    g.addColorStop(1, PAL.helmetBottom);
    x.fillStyle = g;
  }
  x.fill(body);

  // Soft form shading + a glint, so the helmet reads as a glossy shell.
  const sh = x.createRadialGradient(0, 0, R * 0.3, 0, 0, R * 1.3);
  sh.addColorStop(0, "rgba(0,0,0,0)");
  sh.addColorStop(0.65, "rgba(0,0,0,0)");
  sh.addColorStop(1, "rgba(30,0,20,0.28)");
  x.fillStyle = sh;
  x.fill(body);

  const hl = x.createRadialGradient(-rx * 0.5, -ry * 0.55, 0, -rx * 0.5, -ry * 0.55, R * 0.5);
  hl.addColorStop(0, "rgba(255,255,255,0.28)");
  hl.addColorStop(1, "rgba(255,255,255,0)");
  x.fillStyle = hl;
  x.fill(body);
}

/** The cream plate the eyes live on: most of the face, with a thin plum border. */
export function facePlate(
  R: number, yaw: number, pitch: number, morph: number,
): Path2D {
  const shiftX = Math.sin(yaw) * R * 0.08 * (1 - morph);
  const shiftY = -Math.sin(pitch) * R * 0.06 * (1 - morph);
  const hw = lerp(0.93, 0.88, morph) * R;
  const hh = lerp(0.68, 0.82, morph) * R;
  const cy = lerp(0.2, 0.08, morph) * R;
  const p = new Path2D();
  p.roundRect(shiftX - hw, cy + shiftY - hh, hw * 2, hh * 2, lerp(0.42, 0.3, morph) * R);
  return p;
}

export function fillPlate(x: CanvasRenderingContext2D, plate: Path2D, R: number, solid: RGB | null) {
  const g = x.createLinearGradient(0, -R * 0.4, 0, R * 0.8);
  g.addColorStop(0, PAL.plateTop);
  g.addColorStop(1, PAL.plateBottom);
  x.fillStyle = g;
  x.fill(plate);
  x.lineWidth = Math.max(0.6, R * 0.025);
  x.strokeStyle = solid ? rgba(mix(solid, [0, 0, 0], 0.25), 0.5) : PAL.helmetRim;
  x.stroke(plate);
}

export function drawCheeks(
  x: CanvasRenderingContext2D, R: number, yaw: number, amount: number, morph: number,
) {
  if (amount < 0.01) return;
  const a = amount * (1 - morph * 0.8);
  const shift = Math.sin(yaw) * R * 0.14;
  x.fillStyle = `${PAL.cheek}${0.85 * a})`;
  for (const sd of [-1, 1]) {
    x.beginPath();
    x.ellipse(sd * R * 0.62 + shift, R * lerp(0.4, 0.46, morph), R * 0.24, R * 0.15, 0, 0, Math.PI * 2);
    x.fill();
  }
}

// ── Behind the helmet: headphones + scarf ─────────────────────────────────────

export interface BackOpts {
  /** 0…1 visibility (fades out while Mochi becomes a mailbox). */
  vis: number;
  /** 0…1 beat pulse for the ear cups. */
  beat: number;
  yaw: number;
  /** Mini bots recolour the scarf instead of using the default yellow. */
  solid: RGB | null;
}

export function drawBack(
  x: CanvasRenderingContext2D, R: number, _rx: number, ry: number, o: BackOpts,
) {
  if (o.vis < 0.02) return;
  x.save();
  x.globalAlpha = o.vis;

  // Scarf: a soft yellow collar peeking out under the chin, with two stripes.
  const top = ry * 0.7;
  const bottom = ry + R * 0.27;
  const hw = R * 0.92;
  roundRect(x, -hw, top, hw * 2, bottom - top, R * 0.32);
  const sg = x.createLinearGradient(0, top, 0, bottom);
  sg.addColorStop(0, PAL.scarf);
  sg.addColorStop(1, PAL.scarfShade);
  x.fillStyle = sg;
  x.fill();
  x.save();
  x.clip();
  x.fillStyle = PAL.stripeB;
  x.fillRect(-hw, ry + R * 0.03, hw * 2, R * 0.06);
  x.fillStyle = PAL.stripeA;
  x.fillRect(-hw, ry + R * 0.13, hw * 2, R * 0.06);
  x.restore();
  x.restore();
}

/** Ear cups sit in front of the helmet edge, like in the mascot art. */
function drawPhones(x: CanvasRenderingContext2D, R: number, rx: number, beat: number, yaw: number) {
  const sx = 1 + beat * 0.08;
  for (const sd of [-1, 1]) {
    // The cup on the side the head turns away from slides further behind.
    const depth = 1 - 0.18 * Math.max(0, -sd * Math.sin(yaw));
    x.save();
    x.translate(sd * rx * 0.98 + Math.sin(yaw) * R * 0.05, R * 0.36);
    x.scale(sx * depth, 1 + beat * 0.03);
    x.beginPath();
    x.ellipse(sd * R * 0.04, 0, R * 0.25, R * 0.46, 0, 0, Math.PI * 2);
    x.fillStyle = PAL.phoneLight;
    x.fill();
    x.beginPath();
    x.ellipse(-sd * R * 0.025, R * 0.02, R * 0.2, R * 0.4, 0, 0, Math.PI * 2);
    x.fillStyle = PAL.phoneDark;
    x.fill();
    x.restore();
  }
}

// ── In front: cap, tassel, crown ──────────────────────────────────────────────

export interface FrontOpts {
  /** 0…1 visibility of cap + ear cups. The crown never fades. */
  vis: number;
  /** Helmet half-width, for the ear cups. */
  rx: number;
  /** 0…1 beat pulse for the ear cups. */
  beat: number;
  /** Crown rises by this many R while Mochi is a mailbox, so it never covers the slot. */
  lift: number;
  yaw: number;
  pitch: number;
  /** Tassel swing angle (rad), from the engine's spring. */
  swing: number;
  /** Lit amount for each of the five medallions, 0…1. */
  lit: readonly number[];
  glyph: MedalGlyph;
  /** State colour of the lit medallion. */
  color: RGB;
  t: number;
  /** Draw the cap (skipped at tiny sizes where it would clip the island). */
  cap: boolean;
  /** Mini bots get a plain three-point crown. */
  mini: boolean;
  /** Extra crown wobble (rad) for approval jingles etc. */
  wobble: number;
}

export function drawFront(x: CanvasRenderingContext2D, R: number, o: FrontOpts) {
  const par = Math.sin(o.yaw) * R * 0.08;

  if (o.vis > 0.02) {
    x.save();
    x.globalAlpha = o.vis;
    drawPhones(x, R, o.rx, o.beat, o.yaw);
    if (o.cap && !o.mini) drawCap(x, R, par, o);
    x.restore();
  }

  x.save();
  x.translate(par * 0.6, -R * o.lift);
  x.translate(0, -R * 0.4);
  x.rotate(o.wobble);
  x.translate(0, R * 0.4);
  if (o.mini) drawMiniCrown(x, R);
  else drawCrown(x, R, o);
  x.restore();
}

function drawCap(x: CanvasRenderingContext2D, R: number, par: number, o: FrontOpts) {
  x.save();
  x.translate(par * 1.4, 0);
  // A long mortarboard seen from above-left, dipping towards the right.
  const pts: [number, number][] = [
    [-0.98, -1.26], [0.3, -1.22], [1.3, -0.66], [0.0, -0.7],
  ];
  const poly = (dy: number) => {
    x.beginPath();
    pts.forEach(([px, py], i) => (i ? x.lineTo(px * R, (py + dy) * R) : x.moveTo(px * R, (py + dy) * R)));
    x.closePath();
  };
  x.lineJoin = "round";
  x.lineWidth = R * 0.06;
  poly(0.09);
  x.fillStyle = PAL.capUnder;
  x.strokeStyle = PAL.capUnder;
  x.fill();
  x.stroke();
  poly(0);
  const g = x.createLinearGradient(-R, -R * 1.3, R * 1.2, -R * 0.7);
  g.addColorStop(0, PAL.capEdge);
  g.addColorStop(1, PAL.capTop);
  x.fillStyle = g;
  x.strokeStyle = "rgba(190,180,225,0.55)";
  x.lineWidth = Math.max(0.8, R * 0.035);
  x.fill();
  x.stroke();

  // Plum button on the board, cord out to the right corner.
  x.fillStyle = PAL.tassel;
  x.beginPath();
  x.arc(R * 0.62, -R * 0.92, R * 0.09, 0, Math.PI * 2);
  x.fill();

  x.save();
  x.translate(1.2 * R, -0.66 * R);
  x.rotate(o.swing);
  x.strokeStyle = PAL.tassel;
  x.lineWidth = R * 0.07;
  x.lineCap = "round";
  x.beginPath();
  x.moveTo(0, 0);
  x.lineTo(0, R * 0.6);
  x.stroke();
  // Big tassel tuft.
  x.beginPath();
  x.moveTo(-R * 0.12, R * 0.58);
  x.lineTo(R * 0.12, R * 0.58);
  x.lineTo(R * 0.2, R * 1.1);
  x.quadraticCurveTo(0, R * 1.2, -R * 0.2, R * 1.1);
  x.closePath();
  x.fillStyle = PAL.tassel;
  x.fill();
  x.strokeStyle = "rgba(255,255,255,0.25)";
  x.lineWidth = Math.max(0.5, R * 0.025);
  for (const dx of [-0.07, 0, 0.07]) {
    x.beginPath();
    x.moveTo(dx * R, R * 0.7);
    x.lineTo(dx * 1.6 * R, R * 1.12);
    x.stroke();
  }
  x.restore();
  x.restore();
}

const CROWN: readonly (readonly [number, number])[] = [
  [-0.85, -0.38], [-0.9, -1.0], [-0.58, -0.8], [-0.4, -1.04], [-0.17, -0.8],
  [0, -1.28], [0.17, -0.8], [0.4, -1.04], [0.58, -0.8], [0.9, -1.0], [0.85, -0.38],
];

function crownPath(x: CanvasRenderingContext2D, R: number, pts = CROWN) {
  x.beginPath();
  pts.forEach(([px, py], i) => (i ? x.lineTo(px * R, py * R) : x.moveTo(px * R, py * R)));
  x.closePath();
}

function drawCrown(x: CanvasRenderingContext2D, R: number, o: FrontOpts) {
  x.lineJoin = "round";
  x.lineWidth = R * 0.1;
  crownPath(x, R);
  const g = x.createLinearGradient(0, -R * 1.28, 0, -R * 0.38);
  g.addColorStop(0, PAL.goldTop);
  g.addColorStop(1, PAL.goldBottom);
  x.fillStyle = g;
  x.strokeStyle = PAL.goldBottom;
  x.fill();
  x.stroke();

  // Diamond above the centre medallion.
  if (R >= 14) {
    x.fillStyle = "rgba(255,255,255,0.9)";
    x.beginPath();
    x.moveTo(0, -1.12 * R);
    x.lineTo(R * 0.06, -1.03 * R);
    x.lineTo(0, -0.94 * R);
    x.lineTo(-R * 0.06, -1.03 * R);
    x.closePath();
    x.fill();
  }

  for (let i = 0; i < 5; i++) {
    const [mx, my, mr] = MEDALS[i];
    drawMedal(x, R, mx * R, my * R, mr * R, i, o);
  }
}

function drawMiniCrown(x: CanvasRenderingContext2D, R: number) {
  const pts: [number, number][] = [
    [-0.75, -0.4], [-0.8, -1.0], [-0.38, -0.74], [0, -1.22], [0.38, -0.74], [0.8, -1.0], [0.75, -0.4],
  ];
  x.lineJoin = "round";
  x.lineWidth = R * 0.12;
  crownPath(x, R, pts);
  x.fillStyle = PAL.goldTop;
  x.strokeStyle = PAL.goldTop;
  x.fill();
  x.stroke();
}

function drawMedal(
  x: CanvasRenderingContext2D, R: number, cx: number, cy: number, r: number,
  idx: number, o: FrontOpts,
) {
  const lit = o.lit[idx] ?? 0;
  const pulse = 0.78 + 0.22 * Math.sin(o.t * 6 + idx);

  // Tiny sizes: a plain dot is all that survives, and that is enough.
  if (R < 14) {
    x.beginPath();
    x.arc(cx, cy, r * 0.9, 0, Math.PI * 2);
    x.fillStyle = rgba(mix([0.79, 0.56, 0.15], o.color, lit), 1);
    x.fill();
    return;
  }

  if (lit > 0.02) {
    const gr = x.createRadialGradient(cx, cy, r * 0.4, cx, cy, r * 2.4);
    gr.addColorStop(0, rgba(o.color, 0.7 * lit * pulse));
    gr.addColorStop(1, rgba(o.color, 0));
    x.fillStyle = gr;
    x.beginPath();
    x.arc(cx, cy, r * 2.4, 0, Math.PI * 2);
    x.fill();
  }

  // Ring + face.
  x.beginPath();
  x.arc(cx, cy, r, 0, Math.PI * 2);
  x.fillStyle = PAL.goldRim;
  x.fill();
  x.beginPath();
  x.arc(cx, cy, r * 0.82, 0, Math.PI * 2);
  x.fillStyle = lit > 0.02
    ? rgba(mix([0.96, 0.76, 0.31], o.color, Math.min(1, lit * 1.1)), 1)
    : PAL.medalIdle;
  x.fill();

  x.save();
  x.translate(cx, cy);
  const s = r * 0.5;
  x.strokeStyle = lit > 0.3 ? "#fff" : PAL.cream;
  x.fillStyle = x.strokeStyle;
  x.globalAlpha *= 0.62 + 0.38 * lit;
  x.lineWidth = Math.max(0.8, s * 0.26);
  x.lineCap = "round";
  x.lineJoin = "round";

  const glyph = idx === 2 && lit > 0.3 ? o.glyph : idx === 4 && lit > 0.3 && o.glyph === "check" ? "check" : null;
  if (glyph) drawGlyph(x, glyph, s, o.t);
  else drawIcon(x, idx, s, lit, o.t);
  x.restore();
}

function hex(x: CanvasRenderingContext2D, r: number) {
  x.beginPath();
  for (let i = 0; i < 6; i++) {
    const a = Math.PI / 6 + (i * Math.PI) / 3;
    x.lineTo(Math.cos(a) * r, Math.sin(a) * r);
  }
  x.closePath();
}

function drawIcon(x: CanvasRenderingContext2D, idx: number, s: number, lit: number, t: number) {
  switch (idx) {
    case 0: // honeycomb
      hex(x, s * 1.05);
      x.stroke();
      hex(x, s * 0.45);
      x.fill();
      break;
    case 1: { // bolt
      const k = 1 + 0.12 * lit * Math.sin(t * 12);
      x.scale(k, k);
      x.beginPath();
      x.moveTo(s * 0.2, -s * 1.1);
      x.lineTo(-s * 0.7, s * 0.15);
      x.lineTo(-s * 0.05, s * 0.15);
      x.lineTo(-s * 0.25, s * 1.1);
      x.lineTo(s * 0.7, -s * 0.2);
      x.lineTo(s * 0.05, -s * 0.2);
      x.closePath();
      x.fill();
      break;
    }
    case 2: { // bars
      const hs = [0.9, 1.5, 2.0];
      for (let i = 0; i < 3; i++) {
        const bx = (i - 1) * s * 0.62;
        const bh = s * hs[i] * (1 + 0.1 * lit * Math.sin(t * 5 + i * 1.7));
        roundRect(x, bx - s * 0.2, s * 0.8 - bh, s * 0.4, bh, s * 0.12);
        x.fill();
      }
      break;
    }
    case 3: // brain
      x.beginPath();
      x.arc(-s * 0.36, 0, s * 0.62, Math.PI * 0.55, Math.PI * 1.55);
      x.stroke();
      x.beginPath();
      x.arc(s * 0.36, 0, s * 0.62, Math.PI * 1.45, Math.PI * 0.45);
      x.stroke();
      x.beginPath();
      x.moveTo(0, -s * 0.55);
      x.lineTo(0, s * 0.55);
      x.stroke();
      x.beginPath();
      x.moveTo(-s * 0.55, -s * 0.05);
      x.quadraticCurveTo(-s * 0.2, s * 0.3, 0, s * 0.05);
      x.stroke();
      break;
    default: // trend
      x.beginPath();
      x.moveTo(-s * 0.95, s * 0.65);
      x.lineTo(-s * 0.25, -s * 0.05);
      x.lineTo(s * 0.2, s * 0.35);
      x.lineTo(s * 0.9, -s * 0.5);
      x.stroke();
      x.beginPath();
      x.moveTo(s * 0.35, -s * 0.55);
      x.lineTo(s * 0.92, -s * 0.55);
      x.lineTo(s * 0.92, s * 0.02);
      x.stroke();
  }
}

function drawGlyph(x: CanvasRenderingContext2D, g: Exclude<MedalGlyph, null>, s: number, t: number) {
  switch (g) {
    case "bang": {
      const k = 1 + 0.08 * Math.sin(t * 9);
      x.scale(k, k);
      roundRect(x, -s * 0.18, -s * 1.0, s * 0.36, s * 1.4, s * 0.18);
      x.fill();
      x.beginPath();
      x.arc(0, s * 0.8, s * 0.22, 0, Math.PI * 2);
      x.fill();
      break;
    }
    case "question":
      x.beginPath();
      x.arc(0, -s * 0.35, s * 0.55, Math.PI * 1.05, Math.PI * 2.45);
      x.quadraticCurveTo(s * 0.4, s * 0.2, 0, s * 0.35);
      x.stroke();
      x.beginPath();
      x.arc(0, s * 0.85, s * 0.2, 0, Math.PI * 2);
      x.fill();
      break;
    case "cross":
      x.beginPath();
      x.moveTo(-s * 0.7, -s * 0.7);
      x.lineTo(s * 0.7, s * 0.7);
      x.moveTo(s * 0.7, -s * 0.7);
      x.lineTo(-s * 0.7, s * 0.7);
      x.stroke();
      break;
    case "check":
      x.beginPath();
      x.moveTo(-s * 0.8, 0);
      x.lineTo(-s * 0.25, s * 0.6);
      x.lineTo(s * 0.85, -s * 0.6);
      x.stroke();
      break;
  }
}
