import { mount } from "@pocketjs/framework/solid";
import Screen from "./Screen";
import { updateFrame } from "./Status";
mount(() => <Screen />);
const nativeFrame = (globalThis as any).frame;
(globalThis as any).frame = (...args: unknown[]) => {
  (globalThis as any).__pocketProfile?.(0);
  updateFrame();
  (globalThis as any).__pocketProfile?.(3);
  const result = nativeFrame(...args);
  (globalThis as any).__pocketProfile?.(4);
  return result;
};
