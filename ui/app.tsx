import { mount } from "@pocketjs/framework/solid";
import Screen from "./Screen";
import { updateFrame } from "./Status";
mount(() => <Screen />);
const nativeFrame = (globalThis as any).frame;
(globalThis as any).frame = (...args: unknown[]) => {
  updateFrame();
  return nativeFrame(...args);
};
