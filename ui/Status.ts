import { batch, createSignal } from "solid-js";

interface Model {
  link: string; usb: string; layer: number; key_labels: string[];
  modifiers: number; presses: number;
  battery_left: number; battery_right: number;
  charging_left: boolean; charging_right: boolean;
}
const initial: Model = { link: "Searching", usb: "Offline", layer: 0,
  key_labels: ["", "", "", "", "", ""], modifiers: 0, presses: 0,
  battery_left: -1, battery_right: -1, charging_left: false, charging_right: false };
const [state, setState] = createSignal(initial);
const [typing, setTyping] = createSignal(false);
const [phase, setPhase] = createSignal(0);
const [fills, setFills] = createSignal([0, 0]);
export { typing };
export const link = () => state().link;
export const usb = () => state().usb;
export const layer = () => state().layer;
export const keys = () => state().key_labels.filter(label => label.length > 0).length;
export const ctrl = () => (state().modifiers & 0x11) !== 0;
export const shift = () => (state().modifiers & 0x22) !== 0;
export const alt = () => (state().modifiers & 0x44) !== 0;
export const gui = () => (state().modifiers & 0x88) !== 0;
export const key0 = () => state().key_labels[0];
export const key1 = () => state().key_labels[1];
export const key2 = () => state().key_labels[2];
export const key3 = () => state().key_labels[3];
export const key4 = () => state().key_labels[4];
export const key5 = () => state().key_labels[5];
export const battery_left = () => state().battery_left;
export const battery_right = () => state().battery_right;
export const charging_left = () => state().charging_left;
export const charging_right = () => state().charging_right;
export const battery_left_fill = () => fills()[0];
export const battery_right_fill = () => fills()[1];
export const layer_offset = () => Math.trunc(-8 * phase() * phase());
export const layer_opacity = () => 1 - 0.4 * phase();
let started = false, settleUntil = 0, typingUntil = 0, layerStart = -1000;
let fillFrom = [0, 0], fillStart = [0, 0];
const target = (level: number) => Math.max(0, Math.min(100, level)) / 100;
const host = globalThis as any;
host.__rmkPending = null;
host.__rmkTime = 0;
host.__rmkAnimating = true;

export function updateFrame() {
  const now = host.__rmkTime as number;
  const old = state();
  const next: Model = host.__rmkPending ?? old;
  host.__rmkPending = null;
  if (!started) settleUntil = now + 650;
  else {
    if (next.presses !== old.presses || next.key_labels.some((label, i) => label && label !== old.key_labels[i]))
      typingUntil = now + 500;
    if (next.layer !== old.layer) layerStart = now;
    if (next.link !== old.link || next.usb !== old.usb || next.modifiers !== old.modifiers ||
        next.battery_left !== old.battery_left || next.battery_right !== old.battery_right ||
        next.charging_left !== old.charging_left || next.charging_right !== old.charging_right ||
        next.key_labels.some((label, i) => Boolean(label) !== Boolean(old.key_labels[i])))
      settleUntil = Math.max(settleUntil, now + 250);
  }
  const nextTyping = now < typingUntil;
  if (typing() !== nextTyping) settleUntil = Math.max(settleUntil, now + 250);
  const oldLevels = [old.battery_left, old.battery_right];
  const levels = [next.battery_left, next.battery_right];
  const values = levels.map((level, i) => {
    const previous = target(oldLevels[i]), goal = target(level);
    const value = !started ? goal : previous + (fillFrom[i] - previous) * Math.pow(0.75, (now - fillStart[i]) / 1000 * 30);
    const result = Math.abs(value - goal) < 0.002 ? goal : value;
    if (!started || level !== oldLevels[i] || result === goal) { fillFrom[i] = result; fillStart[i] = now; }
    return result;
  });
  const nextPhase = Math.max(0, 1 - (now - layerStart) / 300);
  batch(() => { setState(next); setTyping(nextTyping); setPhase(nextPhase); setFills(values); });
  host.__rmkAnimating = now < settleUntil || nextTyping || nextPhase > 0 ||
    next.link !== "Connected" || next.charging_left || next.charging_right ||
    values.some((value, i) => value !== target(levels[i]));
  started = true;
}
