import { mergeProps } from "solid-js";
import type { JSX } from "solid-js";
import type { i32 } from "@pocketjs/framework/solid/std";
import { View } from "@pocketjs/framework/solid/components";

export default function Card(raw: {
  x: i32; y: i32; width: i32; height: i32;
  delayed?: boolean;
  children?: JSX.Element;
}) {
  const props = mergeProps({ delayed: false }, raw);
  return (
    <View
      class={props.delayed
        ? "absolute rounded-[18px] bg-theme-base border-theme-surface1 animate-enter-late"
        : "absolute rounded-[18px] bg-theme-base border-theme-surface1 animate-enter"}
      style={{ insetL: props.x, insetT: props.y, width: props.width, height: props.height }}
    >
      {props.children}
    </View>
  );
}
