import { Show } from "solid-js";
import {
  link, usb, layer, keys, typing, layer_offset, layer_opacity,
  battery_left, battery_right, battery_left_fill, battery_right_fill,
  charging_left, charging_right, ctrl, shift, alt, gui,
  key0, key1, key2, key3, key4, key5,
} from "./Status";

function Modifier(props: { y: number; label: string; active: boolean }) {
  return <view class="absolute left-[20px] w-[48px] h-[28px]" style={{ insetT: props.y }}>
    <view class="absolute w-[48px] h-[28px] bg-[#dce7fc] transition-opacity duration-120"
      style={{ opacity: props.active ? 1 : 0 }} />
    <text class={props.active
      ? "absolute top-[4px] w-[48px] text-base text-center text-[#243b61]"
      : "absolute top-[4px] w-[48px] text-base text-center text-[#a8b2b0]"}>{props.label}</text>
  </view>;
}

function Power(props: { x: number; side: string; level: number; fill: number; charging: boolean }) {
  return <view class="absolute top-[348px] w-[152px] h-[100px]" style={{ insetL: props.x }}>
    <text class="absolute text-4xl text-[#253331]">{props.level < 0 ? "--" : `${props.level}%`}</text>
    <text class="absolute top-[45px] text-base text-[#66736d]">{props.side}</text>
    <Show when={props.charging}><text class="absolute left-[74px] top-[45px] text-base text-[#346b55]">CHG</text></Show>
    <view class="absolute top-[76px] w-[152px] h-[4px] bg-[#d9ddd2]">
      <view class={props.level >= 0 && props.level < 20
        ? "absolute w-[152px] h-[4px] bg-[#ad6a37] origin-left"
        : "absolute w-[152px] h-[4px] bg-[#547969] origin-left"} style={{ scaleX: props.fill }} />
    </view>
    <view class="absolute top-[85px] w-[1px] h-[5px] bg-[#b1b9ad]" />
    <view class="absolute left-[38px] top-[85px] w-[1px] h-[5px] bg-[#b1b9ad]" />
    <view class="absolute left-[76px] top-[85px] w-[1px] h-[5px] bg-[#b1b9ad]" />
    <view class="absolute left-[114px] top-[85px] w-[1px] h-[5px] bg-[#b1b9ad]" />
    <view class="absolute left-[151px] top-[85px] w-[1px] h-[5px] bg-[#b1b9ad]" />
    <Show when={props.charging}>
      <view class="absolute top-[75px] w-[8px] h-[6px] bg-[#91b8a5] animate-paper-charge" />
    </Show>
  </view>;
}

export default function Paperline() {
  const hero = () => key0() || key1() || key2() || key3() || key4() || key5() || "Ready";
  const chord = () => [key0(), key1(), key2(), key3(), key4(), key5()].filter(Boolean).join(" + ");
  return <view class="w-[480px] h-[480px] bg-[#f4f2e9]">
    <view class="absolute w-[88px] h-[480px] bg-[#253331]">
      <text class="absolute left-[18px] top-[15px] text-5xl font-bold text-[#f4f2e9]">m.</text>
      <view class="absolute left-[20px] top-[97px] w-[48px] h-[1px] bg-[#53615b]" />
      <text class="absolute left-[24px] top-[124px] text-base text-[#a8b2b0]">LYR</text>
      <view class="absolute left-[12px] top-[150px] w-[64px] h-[52px]"
        style={{ translateY: layer_offset(), opacity: layer_opacity() }}>
        <text class="w-[64px] text-4xl text-center text-[#f4f2e9]">{layer() < 10 ? `0${layer()}` : `${layer()}`}</text>
      </view>
      <view class="absolute left-[42px] top-[226px] w-[3px] h-[68px] bg-[#53615b]" />
      <view class="absolute left-[36px] top-[226px] w-[15px] h-[6px] bg-[#a9c3ff] transition-transform duration-200"
        style={{ translateY: Math.min(layer(), 3) * 20 }} />
      <Modifier y={333} label="CTL" active={ctrl()} />
      <Modifier y={365} label="SFT" active={shift()} />
      <Modifier y={397} label="ALT" active={alt()} />
      <Modifier y={429} label="GUI" active={gui()} />
    </view>

    <text class="absolute left-[112px] top-[28px] text-base font-mono text-[#253331]">PAPERLINE</text>
    <view class={link() === "Connected"
      ? "absolute left-[336px] top-[35px] w-[6px] h-[6px] rounded-full bg-[#547969]"
      : "absolute left-[336px] top-[35px] w-[6px] h-[6px] rounded-full bg-[#ad6a37] animate-battery-pulse"} />
    <text class="absolute left-[350px] top-[28px] text-base text-[#547969]">{link()}</text>
    <view class="absolute left-[112px] top-[68px] w-[344px] h-[1px] bg-[#cdd3c7]" />

    <text class="absolute left-[112px] top-[96px] text-base text-[#66736d]">KEYSTROKES</text>
    <text class="absolute left-[333px] top-[96px] w-[123px] text-base text-right text-[#66736d]">
      {keys() ? `${keys()} HELD` : "AT REST"}
    </text>
    <view class="absolute left-[112px] top-[142px] w-[344px] h-[86px] overflow-hidden">
      <text class={keys()
        ? "text-5xl font-bold text-[#3d61b7]"
        : "text-5xl font-bold text-[#253331]"}>{hero()}</text>
    </view>
    <view class="absolute left-[112px] top-[229px] w-[344px] h-[2px] bg-[#253331]" />
    <Show when={typing()}>
      <view class="absolute left-[112px] top-[229px] w-[24px] h-[2px] bg-[#7d9cdf] animate-paper-ink" />
    </Show>
    <text class="absolute left-[112px] top-[250px] w-[344px] h-[56px] text-xl text-[#66736d]">
      {chord() || "A quiet moment."}
    </text>
    <view class="absolute left-[112px] top-[322px] w-[344px] h-[1px] bg-[#cdd3c7]" />
    <Power x={112} side="LEFT" level={battery_left()} fill={battery_left_fill()} charging={charging_left()} />
    <Power x={304} side="RIGHT" level={battery_right()} fill={battery_right_fill()} charging={charging_right()} />
    <text class="absolute left-[112px] top-[455px] text-sm text-[#66736d]">{`USB / ${usb()}`}</text>
    <text class="absolute left-[355px] top-[455px] text-sm font-mono text-[#66736d]">MOSAICO</text>
  </view>;
}
