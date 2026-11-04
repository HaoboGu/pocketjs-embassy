import { createSignal } from "solid-js";
import { mount } from "@pocketjs/framework/solid";
import { View, Text } from "@pocketjs/framework/solid/components";

const [count, setCount] = createSignal(0);
(globalThis as any).setKeyCount = (value: number) => setCount(value);

function App() {
  return (
    <View class="w-full h-full bg-[#ffffff] p-8 flex-col gap-6">
      <Text class="text-2xl font-bold text-[#1f2328]">PocketJS + Embassy</Text>
      <Text class="text-base text-[#59636e]">Solid / QuickJS / no_std Rust</Text>
      <View class="bg-[#ddf4ff] rounded-xl p-8 flex-col gap-4">
        <Text class="text-xl text-[#0969da]">KEYS HELD</Text>
        <Text class="text-2xl font-bold text-[#0969da]">{count()}</Text>
      </View>
      <Text class="text-lg text-[#1a7f37]">JavaScript is running</Text>
    </View>
  );
}
mount(() => <App />);
