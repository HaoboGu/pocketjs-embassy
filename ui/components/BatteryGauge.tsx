import { Show } from "solid-js";
import type { i32, f32 } from "@pocketjs/framework/solid/std";
import Card from "./Card.tsx";

export default function BatteryGauge(props: {
  x: i32; y: i32; label: string; level: i32; fill: f32; charging: boolean;
}) {
  return (
    <Card x={props.x} y={props.y} width={196} height={84} delayed={true}>
      <view class={props.charging
        ? "absolute left-[16px] top-[18px] w-[22px] h-[12px] rounded-[3px] border-theme-teal"
        : "absolute left-[16px] top-[18px] w-[22px] h-[12px] rounded-[3px] border-theme-subtext0"}>
        <view class={props.charging
          ? "absolute left-[3px] top-[3px] w-[16px] h-[6px] rounded-[1px] bg-theme-teal origin-left animate-battery-pulse"
          : "absolute left-[3px] top-[3px] w-[16px] h-[6px] rounded-[1px] bg-theme-subtext0 origin-left"}
          style={{ scaleX: props.fill }} />
      </view>
      <view class={props.charging
        ? "absolute left-[38px] top-[21px] w-[2px] h-[6px] bg-theme-teal"
        : "absolute left-[38px] top-[21px] w-[2px] h-[6px] bg-theme-subtext0"} />
      <text class="absolute left-[48px] top-[17px] text-base text-theme-subtext0">{props.label}</text>
      <text class={props.level >= 0 && props.level <= 20 && !props.charging
        ? "absolute left-[104px] top-[12px] w-[76px] text-2xl text-right text-theme-yellow transition-colors duration-200"
        : "absolute left-[104px] top-[12px] w-[76px] text-2xl text-right text-theme-text transition-colors duration-200"}
      >{props.level < 0 ? "--" : `${props.level}%`}</text>
      <text class={props.charging
        ? "absolute left-[16px] top-[37px] text-base text-theme-teal transition-colors duration-200"
        : props.level >= 0 && props.level <= 20
        ? "absolute left-[16px] top-[37px] text-base text-theme-yellow transition-colors duration-200"
        : "absolute left-[16px] top-[37px] text-base text-theme-overlay1 transition-colors duration-200"}
      >{props.charging ? "Charging" : props.level < 0 ? "No reading" : props.level <= 20 ? "Low battery" : "Battery"}</text>
      <view class="absolute left-[16px] top-[60px] w-[164px] h-[6px] rounded-full bg-theme-mantle overflow-hidden">
        <view class={props.level <= 20 && !props.charging
          ? "absolute w-[164px] h-[6px] rounded-[3px] bg-theme-yellow origin-left transition-colors duration-200"
          : "absolute w-[164px] h-[6px] rounded-[3px] bg-theme-teal origin-left transition-colors duration-200"}
          style={{ scaleX: props.fill }} />
        <Show when={props.charging}>
          <view class="absolute top-[1px] w-[22px] h-[4px] rounded-[2px] bg-theme-rosewater animate-charge" />
        </Show>
      </view>
    </Card>
  );
}
