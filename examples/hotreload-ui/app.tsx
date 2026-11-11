import { createMemo } from "solid-js";
import { mount } from "@pocketjs/framework/solid";
import { View, Text } from "@pocketjs/framework/components";
import type { f32, i32 } from "@pocketjs/framework/solid/std";
import {
  updateFrame, link, usb, layer, keys, typing, layer_offset, layer_opacity,
  key0, key1, key2, key3, key4, key5, ctrl, shift, alt, gui,
  battery_left, battery_right, battery_left_fill, battery_right_fill,
  charging_left, charging_right,
} from "../../ui/Status";

const primaryKey = createMemo(() => key0() || key1() || key2() || key3() || key4() || key5()
  || (ctrl() ? "CTRL" : shift() ? "SHIFT" : alt() ? "ALT" : gui() ? "GUI" : "READY"));
const activity = createMemo(() => keys() > 0 ? `${keys()} ${keys() === 1 ? "key" : "keys"} held`
  : ctrl() || shift() || alt() || gui() ? "Modifier held" : typing() ? "Key activity" : "Ready for input");

function KeySlot(props: { x: i32; label: string }) {
  return (
    <View class={props.label
      ? "absolute top-[306px] w-[64px] h-[38px] rounded-[6px] border-[#B4D2BE] bg-[#DFF0E5]"
      : "absolute top-[306px] w-[64px] h-[38px] rounded-[6px] border-[#DDE2D8] bg-[#F0F1EB]"}
      style={{ insetL: props.x }}>
      <Text class="absolute top-[7px] w-[64px] text-lg text-center text-[#245845]">{props.label || " "}</Text>
    </View>
  );
}

function Modifier(props: { x: i32; label: string; active: boolean }) {
  return (
    <View class="absolute top-[361px] w-[100px] h-[31px]" style={{ insetL: props.x }}>
      <Text class={props.active
        ? "w-[100px] text-lg text-center font-bold text-[#2B7E61]"
        : "w-[100px] text-lg text-center text-[#8A968D]"}>{props.label}</Text>
      <View class="absolute left-[36px] top-[27px] w-[28px] h-[2px] bg-[#2B7E61]"
        style={{ opacity: props.active ? 1 : 0 }} />
    </View>
  );
}

function Power(props: { x: i32; label: string; level: i32; fill: f32; charging: boolean }) {
  return (
    <View class="absolute top-[407px] w-[200px] h-[46px]" style={{ insetL: props.x }}>
      <Text class="absolute top-[6px] text-lg text-[#65766D]">{props.charging ? `${props.label} / CHG` : props.label}</Text>
      <Text class={props.level >= 0 && props.level <= 20 && !props.charging
        ? "absolute right-[0px] w-[72px] text-2xl font-bold text-right text-[#A47024]"
        : "absolute right-[0px] w-[72px] text-2xl font-bold text-right text-[#173B36]"}>
        {props.level < 0 ? "--" : `${props.level}%`}
      </Text>
      <View class="absolute top-[41px] w-[200px] h-[5px] rounded-[2px] overflow-hidden bg-[#DDE5DB]">
        <View class={props.charging
          ? "absolute w-[200px] h-[5px] bg-[#2B7E61] origin-left animate-signal-pulse"
          : props.level >= 0 && props.level <= 20
          ? "absolute w-[200px] h-[5px] bg-[#A47024] origin-left"
          : "absolute w-[200px] h-[5px] bg-[#2B7E61] origin-left"}
          style={{ scaleX: props.fill }} />
      </View>
    </View>
  );
}

function Screen() {
  return (
    <View class="w-[480px] h-[480px] bg-[#F7F5EF]">
      <View class="absolute w-[480px] h-[76px] bg-[#173B36]">
        <View class="absolute left-[28px] top-[27px] w-[5px] h-[23px] bg-[#8FC6A5]" />
        <View class="absolute left-[37px] top-[19px] w-[5px] h-[31px] bg-[#DFF0E5]" />
        <Text class="absolute left-[60px] top-[12px] text-2xl font-bold text-[#FFFFFF]">SIGNAL</Text>
        <Text class="absolute left-[61px] top-[42px] text-lg text-[#C5D6CB]">MOSAICO</Text>
        <View class={link() === "Connected"
          ? "absolute left-[330px] top-[32px] w-[6px] h-[6px] rounded-full bg-[#9AD8AF]"
          : "absolute left-[330px] top-[32px] w-[6px] h-[6px] rounded-full bg-[#E4C380] animate-signal-pulse"} />
        <Text class="absolute left-[348px] top-[23px] w-[112px] text-lg text-[#FFFFFF]">{link()}</Text>
      </View>

      <Text class="absolute left-[28px] top-[97px] text-lg text-[#65766D]">LAYER</Text>
      <View class="absolute left-[98px] top-[91px] w-[60px] h-[36px]"
        style={{ translateY: layer_offset(), opacity: layer_opacity() }}>
        <Text class="text-2xl font-bold text-[#173B36]">{layer() < 10 ? `0${layer()}` : `${layer()}`}</Text>
      </View>
      <Text class="absolute left-[266px] top-[97px] w-[186px] text-lg text-right text-[#65766D]">{`USB / ${usb()}`}</Text>
      <View class="absolute left-[28px] top-[135px] w-[424px] h-[1px] bg-[#D7DDD5]" />

      <Text class="absolute top-[150px] w-[480px] text-lg text-center text-[#65766D]">CURRENT INPUT</Text>
      <View class="absolute left-[28px] top-[176px] w-[424px] h-[88px] animate-signal-arrive">
        <Text class={primaryKey().length > 4
          ? "absolute top-[25px] w-[424px] text-4xl font-bold text-center text-[#173B36]"
          : "absolute top-[10px] w-[424px] text-5xl font-bold text-center text-[#173B36]"}>{primaryKey()}</Text>
      </View>
      <Text class="absolute top-[268px] w-[480px] text-lg text-center text-[#65766D]">{activity()}</Text>
      <View class="absolute left-[220px] top-[293px] w-[40px] h-[3px] bg-[#2B7E61] origin-center transition-transform duration-150 ease-out"
        style={{ scaleX: typing() || keys() > 0 || ctrl() || shift() || alt() || gui() ? 1 : 0.25 }} />

      <KeySlot x={28} label={key0()} /><KeySlot x={100} label={key1()} />
      <KeySlot x={172} label={key2()} /><KeySlot x={244} label={key3()} />
      <KeySlot x={316} label={key4()} /><KeySlot x={388} label={key5()} />
      <Modifier x={28} label="CTRL" active={ctrl()} /><Modifier x={136} label="SHIFT" active={shift()} />
      <Modifier x={244} label="ALT" active={alt()} /><Modifier x={352} label="GUI" active={gui()} />
      <Power x={28} label="LEFT" level={battery_left()} fill={battery_left_fill()} charging={charging_left()} />
      <Power x={252} label="RIGHT" level={battery_right()} fill={battery_right_fill()} charging={charging_right()} />
    </View>
  );
}

mount(() => <Screen />);
const host = globalThis as any;
const nativeFrame = host.frame;
host.frame = (...args: unknown[]) => {
  updateFrame();
  return nativeFrame(...args);
};
