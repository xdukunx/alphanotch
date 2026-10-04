// Dev harness: every state, emote and the mailbox morph side by side, so the
// character can be checked without driving the whole island. Click a cell to
// replay its animation. Not part of the app bundle.

import { BotEngine } from "../src/mochi/engine";
import type { BotEmoteName, BotStateName } from "../src/core/layout";

const STATES: BotStateName[] = [
  "idle", "working", "thinking", "searching", "approval", "question",
  "error", "finished", "ratelimit", "sleeping", "dizzy",
];
const EMOTES: BotEmoteName[] = ["love", "surprised", "proud", "wink", "yawn", "happy", "annoyed"];

const W = 190;
const H = 230;
const dpr = Math.min(2, window.devicePixelRatio || 1);
const grid = document.getElementById("grid")!;
const cells: { engine: BotEngine; ctx: CanvasRenderingContext2D }[] = [];

function cell(label: string, setup: (e: BotEngine) => void) {
  const el = document.createElement("div");
  el.className = "cell";
  const c = document.createElement("canvas");
  c.width = W * dpr;
  c.height = H * dpr;
  c.style.width = `${W}px`;
  c.style.height = `${H}px`;
  el.append(c, label);
  grid.append(el);
  const engine = new BotEngine();
  engine.particleOverhang = 80;
  engine.lookX = 0.2;
  const ctx = c.getContext("2d")!;
  setup(engine);
  el.onclick = () => setup(engine);
  cells.push({ engine, ctx });
}

for (const s of STATES) cell(s, (e) => e.setState(s, true));
for (const m of EMOTES) cell(`emote: ${m}`, (e) => { e.setState("idle", true); e.triggerEmote(m, 4); });
cell("mailbox", (e) => { e.setState("idle", true); e.animateMorph(1); });
cell("greet / wave", (e) => { e.setState("idle", true); e.animateMorph(0); e.greet(); });

let last = performance.now();
function frame(now: number) {
  const dt = Math.min(0.05, (now - last) / 1000);
  last = now;
  for (const { engine, ctx } of cells) {
    engine.update(dt);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    // The island gives the canvas 80 px of headroom; the body is 0.6 of its width.
    engine.draw(ctx, W * 1.0, H);
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
