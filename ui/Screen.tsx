import { Text, View } from "@pocketjs/framework/solid/components";
import Card from "./components/Card.tsx";
import StatusBadge from "./components/StatusBadge.tsx";
import Keycap from "./components/Keycap.tsx";
import BatteryGauge from "./components/BatteryGauge.tsx";
import ActivityMeter from "./components/ActivityMeter.tsx";
import {
  link, usb, layer, keys, typing, layer_offset, layer_opacity,
  battery_left, battery_right, battery_left_fill, battery_right_fill,
  charging_left, charging_right, ctrl, shift, alt, gui,
  key0, key1, key2, key3, key4, key5,
} from "./Status";

export default function Status() {
  return (
    <View class="w-[480px] h-[480px] bg-theme-canvas">
      <View class="absolute left-[32px] top-[22px] w-[416px] h-[64px] animate-enter">
        <View class="absolute top-[6px] w-[6px] h-[28px] rounded-[3px] bg-theme-mauve" />
        <View class="absolute left-[10px] top-[14px] w-[6px] h-[20px] rounded-[3px] bg-theme-teal" />
        <Text class="absolute left-[26px] top-[0px] text-4xl font-bold text-theme-text">Mosaico</Text>
        <View class={usb() === "Ready"
          ? "absolute left-[1px] top-[54px] w-[6px] h-[6px] rounded-full bg-theme-green transition-colors duration-200"
          : "absolute left-[1px] top-[54px] w-[6px] h-[6px] rounded-full bg-theme-yellow transition-colors duration-200"} />
        <Text class="absolute left-[14px] top-[46px] text-base text-theme-subtext0">{`USB / ${usb()}`}</Text>
        <StatusBadge x={278} y={8} width={138} height={32} label={link()}
          active={link() === "Connected"} warning={link() !== "Connected"} dot={true} />
      </View>

      <Card x={32} y={100} width={416} height={194}>
        <Text class="absolute left-[20px] top-[22px] text-base text-theme-subtext0">LAYER</Text>
        <View class="absolute left-[18px] top-[45px] w-[110px] h-[70px]"
          style={{ translateY: layer_offset(), opacity: layer_opacity() }}>
          <Text class="text-5xl font-bold text-theme-lavender">{layer() < 10 ? `0${layer()}` : `${layer()}`}</Text>
        </View>
        <Text class="absolute left-[20px] top-[123px] text-base text-theme-subtext0">
          {keys() === 0 ? "Ready" : `${keys()} held`}
        </Text>
        <ActivityMeter active={typing()} />
        <View class="absolute left-[136px] top-[24px] w-[1px] h-[146px] bg-theme-surface1" />
        <Text class="absolute left-[160px] top-[22px] text-base text-theme-subtext0">PRESSED KEYS</Text>
        <Keycap x={160} y={52} label={key0()} />
        <Keycap x={240} y={52} label={key1()} />
        <Keycap x={320} y={52} label={key2()} />
        <Keycap x={160} y={112} label={key3()} />
        <Keycap x={240} y={112} label={key4()} />
        <Keycap x={320} y={112} label={key5()} />
      </Card>

      <View class="absolute left-[32px] top-[310px] w-[416px] h-[36px] animate-enter-badge">
        <StatusBadge x={0} y={0} label="CTRL" active={ctrl()} />
        <StatusBadge x={106} y={0} label="SHIFT" active={shift()} />
        <StatusBadge x={212} y={0} label="ALT" active={alt()} />
        <StatusBadge x={318} y={0} label="GUI" active={gui()} />
      </View>
      <BatteryGauge x={32} y={366} label="LEFT" level={battery_left()} fill={battery_left_fill()} charging={charging_left()} />
      <BatteryGauge x={252} y={366} label="RIGHT" level={battery_right()} fill={battery_right_fill()} charging={charging_right()} />
    </View>
  );
}
