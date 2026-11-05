# /// script
# requires-python = ">=3.10"
# dependencies = ["pyusb>=1.3,<2"]
# ///
"""Enter Mosaico ROM download mode before flashing."""

import argparse

import usb.core

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--serial", help="USB serial string, required if several Mosaico dongles are connected")
args = parser.parse_args()
devices = [
    device
    for device in usb.core.find(find_all=True, idVendor=0x4C4B, idProduct=0x4644)
    if args.serial is None or device.serial_number == args.serial
]
if len(devices) != 1:
    parser.error(f"found {len(devices)} matching Mosaico dongles; connect one or specify --serial")
devices[0].ctrl_transfer(0x40, 0x71, 0x4D53, 0, b"", timeout=2000)
print("Requested ROM download mode; use the newly appearing serial port for esptool.")
