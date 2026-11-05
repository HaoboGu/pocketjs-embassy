use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_usb::{
    Handler,
    control::{OutResponse, Recipient, Request, RequestType},
};

pub static REQUESTED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub struct BootloaderHandler;
impl Handler for BootloaderHandler {
    #[cfg(feature = "usb-diagnostics")]
    fn control_in<'a>(
        &'a mut self,
        request: Request,
        buffer: &'a mut [u8],
    ) -> Option<embassy_usb::control::InResponse<'a>> {
        if request.request_type != RequestType::Vendor
            || request.recipient != Recipient::Device
            || request.request != 0x74
            || !matches!(request.value, 0x4d53..=0x4d56 | 0x4d58..=0x4d59)
        {
            return None;
        }
        let limit = usize::from(request.length).min(buffer.len());
        let length = crate::diagnostics::read(request.value, request.index, &mut buffer[..limit]);
        Some(embassy_usb::control::InResponse::Accepted(
            &buffer[..length],
        ))
    }

    fn control_out(&mut self, request: Request, data: &[u8]) -> Option<OutResponse> {
        if request.request_type == RequestType::Vendor
            && request.recipient == Recipient::Device
            && request.request == 0x71
            && request.value == 0x4d53
            && request.index == 0
            && request.length == 0
            && data.is_empty()
        {
            REQUESTED.signal(());
            Some(OutResponse::Accepted)
        } else {
            None
        }
    }
}
