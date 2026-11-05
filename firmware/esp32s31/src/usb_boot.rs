use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_usb::{
    Handler,
    control::{OutResponse, Recipient, Request, RequestType},
};

pub static REQUESTED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub struct BootloaderHandler;
impl Handler for BootloaderHandler {
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
