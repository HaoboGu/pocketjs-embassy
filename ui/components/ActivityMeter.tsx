
export default function ActivityMeter(props: { active: boolean }) {
  return (
    <view class="absolute left-[20px] top-[153px] w-[86px] h-[18px]">
      <view class={props.active
        ? "absolute left-[0px] w-[6px] h-[18px] rounded-[3px] bg-theme-teal origin-bottom opacity-100 animate-meter-a"
        : "absolute left-[0px] w-[6px] h-[18px] rounded-[3px] bg-theme-overlay0 origin-bottom scale-y-50 opacity-0 transition duration-240 ease-out"} />
      <view class={props.active
        ? "absolute left-[14px] w-[6px] h-[18px] rounded-[3px] bg-theme-mauve origin-bottom opacity-100 animate-meter-b"
        : "absolute left-[14px] w-[6px] h-[18px] rounded-[3px] bg-theme-overlay0 origin-bottom scale-y-50 opacity-0 transition duration-240 ease-out"} />
      <view class={props.active
        ? "absolute left-[28px] w-[6px] h-[18px] rounded-[3px] bg-theme-teal origin-bottom opacity-100 animate-meter-c"
        : "absolute left-[28px] w-[6px] h-[18px] rounded-[3px] bg-theme-overlay0 origin-bottom scale-y-50 opacity-0 transition duration-240 ease-out"} />
      <view class={props.active
        ? "absolute left-[42px] w-[6px] h-[18px] rounded-[3px] bg-theme-mauve origin-bottom opacity-100 animate-meter-a"
        : "absolute left-[42px] w-[6px] h-[18px] rounded-[3px] bg-theme-overlay0 origin-bottom scale-y-50 opacity-0 transition duration-240 ease-out"} />
      <view class={props.active
        ? "absolute left-[56px] w-[6px] h-[18px] rounded-[3px] bg-theme-teal origin-bottom opacity-100 animate-meter-b"
        : "absolute left-[56px] w-[6px] h-[18px] rounded-[3px] bg-theme-overlay0 origin-bottom scale-y-50 opacity-0 transition duration-240 ease-out"} />
      <view class={props.active
        ? "absolute left-[70px] w-[6px] h-[18px] rounded-[3px] bg-theme-mauve origin-bottom opacity-100 animate-meter-c"
        : "absolute left-[70px] w-[6px] h-[18px] rounded-[3px] bg-theme-overlay0 origin-bottom scale-y-50 opacity-0 transition duration-240 ease-out"} />
    </view>
  );
}
