import { mergeProps, Show } from "solid-js";
import { idiv } from "@pocketjs/framework/solid/std";
import type { i32 } from "@pocketjs/framework/solid/std";
import { Text, View } from "@pocketjs/framework/solid/components";

export default function StatusBadge(raw: {
  x: i32;
  y: i32;
  label: string;
  active: boolean;
  width?: i32;
  height?: i32;
  warning?: boolean;
  dot?: boolean;
}) {
  const props = mergeProps({ width: 98, height: 36, warning: false, dot: false }, raw);
  return (
    <View
      class={props.warning
        ? "absolute rounded-[10px] bg-theme-surface0 border-theme-yellow transition-colors duration-200 ease-out"
        : props.active
        ? props.dot
          ? "absolute rounded-[10px] bg-theme-activebg border-theme-teal transition-colors duration-200 ease-out"
          : "absolute rounded-[10px] bg-theme-keybg border-theme-keyaccent"
        : props.dot
          ? "absolute rounded-[10px] bg-theme-base border-theme-surface1 transition-colors duration-200 ease-out"
          : "absolute rounded-[10px] bg-theme-base border-theme-surface1"}
      style={{ insetL: props.x, insetT: props.y, width: props.width, height: props.height }}
    >
      <Show when={props.dot}>
        <View
          class={props.warning
            ? "absolute left-[14px] w-[6px] h-[6px] rounded-full bg-theme-yellow animate-breathe"
            : props.active
            ? "absolute left-[14px] w-[6px] h-[6px] rounded-full bg-theme-green"
            : "absolute left-[14px] w-[6px] h-[6px] rounded-full bg-theme-overlay1"}
          style={{ insetT: idiv(props.height - 6, 2) }}
        />
      </Show>
      <View class="absolute"
        style={{
          insetL: props.dot ? 29 : 0,
          insetT: idiv(props.height - 20, 2),
          width: props.dot ? props.width - 36 : props.width,
          height: 20,
        }}
      >
        <Text class={props.dot
          ? props.warning
            ? "w-full text-base text-theme-yellow"
            : props.active ? "w-full text-base text-theme-green" : "w-full text-base text-theme-overlay1"
          : props.warning
            ? "w-full text-base font-bold text-center text-theme-yellow"
            : props.active ? "w-full text-base font-bold text-center text-theme-keyaccent"
            : "w-full text-base font-bold text-center text-theme-overlay1"}
        >{props.label}</Text>
      </View>
    </View>
  );
}
