import { batch, createMemo, createSignal } from "solid-js";

interface Model {
  link: string; usb: string; layer: number; key_labels: string[];
  modifiers: number; presses: number;
  battery_left: number; battery_right: number;
  charging_left: boolean; charging_right: boolean;
}
const initial: Model = { link: "Searching", usb: "Offline", layer: 0,
  key_labels: ["", "", "", "", "", ""], modifiers: 0, presses: 0,
  battery_left: -1, battery_right: -1, charging_left: false, charging_right: false };
let state = initial;
const [link, setLink] = createSignal(initial.link);
const [usb, setUsb] = createSignal(initial.usb);
const [layer, setLayer] = createSignal(initial.layer);
const [keys, setKeys] = createSignal(0);
const [ctrl, setCtrl] = createSignal(false);
const [shift, setShift] = createSignal(false);
const [alt, setAlt] = createSignal(false);
const [gui, setGui] = createSignal(false);
const [key0, setKey0] = createSignal("");
const [key1, setKey1] = createSignal("");
const [key2, setKey2] = createSignal("");
const [key3, setKey3] = createSignal("");
const [key4, setKey4] = createSignal("");
const [key5, setKey5] = createSignal("");
const [battery_left, setBatteryLeft] = createSignal(-1);
const [battery_right, setBatteryRight] = createSignal(-1);
const [charging_left, setChargingLeft] = createSignal(false);
const [charging_right, setChargingRight] = createSignal(false);
const [battery_left_fill, setLeftFill] = createSignal(0);
const [battery_right_fill, setRightFill] = createSignal(0);
const [typing, setTyping] = createSignal(false);
const [phase, setPhase] = createSignal(0);
export { link, usb, layer, keys, ctrl, shift, alt, gui, key0, key1, key2, key3, key4, key5,
  battery_left, battery_right, charging_left, charging_right, battery_left_fill, battery_right_fill, typing };
export const layer_offset = createMemo(() => Math.trunc(-8 * phase() * phase()));
export const layer_opacity = createMemo(() => 1 - 0.4 * phase());
let started = false, settleUntil = 0, typingUntil = 0, layerStart = -1000;
let fillFrom = [0, 0], fillStart = [0, 0];
const target = (level: number) => Math.max(0, Math.min(100, level)) / 100;
const host = globalThis as any;
host.__rmkPending = null;
host.__rmkTime = 0;
host.__rmkAnimating = true;
host.__rmkNextFrame = 0;

export function updateFrame() {
  const now = host.__rmkTime as number;
  const old = state;
  const next: Model = host.__rmkPending ?? old;
  host.__rmkPending = null;
  const changed = next !== old;
  let count = keys(), newPress = next.presses !== old.presses, presenceChanged = false;
  if (changed) {
    count = 0;
    for (let i = 0; i < 6; i++) {
      const label = next.key_labels[i], previous = old.key_labels[i];
      if (label.length) { count++; if (label !== previous) newPress = true; }
      if (!!label !== !!previous) presenceChanged = true;
    }
  }
  if (!started) settleUntil = now + 650;
  else {
    if (newPress) typingUntil = now + 500;
    if (next.layer !== old.layer) layerStart = now;
    if (next.link !== old.link || next.usb !== old.usb || next.modifiers !== old.modifiers ||
        next.battery_left !== old.battery_left || next.battery_right !== old.battery_right ||
        next.charging_left !== old.charging_left || next.charging_right !== old.charging_right || presenceChanged)
      settleUntil = Math.max(settleUntil, now + 250);
  }
  const nextTyping = now < typingUntil;
  if (typing() !== nextTyping) settleUntil = Math.max(settleUntil, now + 250);
  let left = battery_left_fill(), right = battery_right_fill(), filling = false;
  for (let i = 0; i < 2; i++) {
    const level = i === 0 ? next.battery_left : next.battery_right;
    const oldLevel = i === 0 ? old.battery_left : old.battery_right;
    const current = i === 0 ? left : right;
    const goal = target(level);
    if (started && oldLevel === level && current === goal) continue;
    const previous = target(oldLevel);
    const value = !started ? goal : fillFrom[i] === previous ? previous : previous + (fillFrom[i] - previous) * Math.pow(0.75, (now - fillStart[i]) / 1000 * 30);
    const result = Math.abs(value - goal) < 0.002 ? goal : value;
    if (!started || level !== oldLevel || result === goal) { fillFrom[i] = result; fillStart[i] = now; }
    if (i === 0) left = result; else right = result;
    if (result !== goal) filling = true;
  }
  const elapsed = now - layerStart;
  const nextPhase = elapsed >= 300 ? 0 : 1 - elapsed / 300;
  batch(() => {
    if (changed) {
      if (next.link !== old.link) setLink(next.link);
      if (next.usb !== old.usb) setUsb(next.usb);
      if (next.layer !== old.layer) setLayer(next.layer);
      setKeys(count);
      if (next.modifiers !== old.modifiers) {
        setCtrl((next.modifiers & 0x11) !== 0); setShift((next.modifiers & 0x22) !== 0);
        setAlt((next.modifiers & 0x44) !== 0); setGui((next.modifiers & 0x88) !== 0);
      }
      if (next.key_labels[0] !== old.key_labels[0]) setKey0(next.key_labels[0]);
      if (next.key_labels[1] !== old.key_labels[1]) setKey1(next.key_labels[1]);
      if (next.key_labels[2] !== old.key_labels[2]) setKey2(next.key_labels[2]);
      if (next.key_labels[3] !== old.key_labels[3]) setKey3(next.key_labels[3]);
      if (next.key_labels[4] !== old.key_labels[4]) setKey4(next.key_labels[4]);
      if (next.key_labels[5] !== old.key_labels[5]) setKey5(next.key_labels[5]);
      if (next.battery_left !== old.battery_left) setBatteryLeft(next.battery_left);
      if (next.battery_right !== old.battery_right) setBatteryRight(next.battery_right);
      if (next.charging_left !== old.charging_left) setChargingLeft(next.charging_left);
      if (next.charging_right !== old.charging_right) setChargingRight(next.charging_right);
    }
    setTyping(nextTyping); setPhase(nextPhase);
    setLeftFill(left); setRightFill(right);
  });
  state = next;
  const easing = nextPhase > 0 || filling;
  host.__rmkAnimating = now < settleUntil || nextTyping || easing ||
    next.link !== "Connected" || next.charging_left || next.charging_right;
  host.__rmkNextFrame = easing ? now : Math.min(nextTyping ? typingUntil : Infinity,
    now < settleUntil ? settleUntil : Infinity);
  started = true;
}
