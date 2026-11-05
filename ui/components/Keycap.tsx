import { len } from "@pocketjs/framework/solid/std";
import type { i32 } from "@pocketjs/framework/solid/std";

export default function Keycap(props: { x: i32; y: i32; label: string }) {
  return (
    <view class="absolute w-[72px] h-[52px]" style={{ insetL: props.x, insetT: props.y }}>
      <view class="absolute top-[4px] w-[72px] h-[48px] rounded-[10px] bg-theme-crust" />
      <view class={len(props.label) > 0
        ? "absolute w-[72px] h-[48px] rounded-[10px] bg-theme-keybg border-theme-keyaccent translate-y-[3px] scale-100 transition-transform duration-140 ease-out"
        : "absolute w-[72px] h-[48px] rounded-[10px] bg-theme-mantle border-theme-surface1 translate-y-[0px] scale-100 transition-transform duration-200 ease-out-back"}
      >
        <text class={len(props.label) === 0
          ? "absolute left-[2px] top-[12px] w-[68px] text-xl text-center text-theme-overlay1"
          : len(props.label) > 3
          ? "absolute left-[2px] top-[13px] w-[68px] text-lg text-center text-theme-text"
          : "absolute left-[2px] top-[9px] w-[68px] text-2xl text-center text-theme-text"}
        >{len(props.label) === 0 ? "-" : props.label}</text>
      </view>
    </view>
  );
}
