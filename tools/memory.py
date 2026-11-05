"""Report the linked S31 internal-memory layout; this is not a live heap measurement."""
import json
from pathlib import Path
import re
import subprocess
import sys

root = Path(__file__).resolve().parent.parent
elf = Path(sys.argv[1])
tools = root / ".deps/riscv32-esp-elf/bin"
sections = {}
for line in subprocess.check_output([str(tools / "riscv32-esp-elf-objdump"), "-h", str(elf)], text=True).splitlines():
    match = re.match(r"\s*\d+\s+(\S+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s", line)
    if match:
        sections[match[1]] = {"bytes": int(match[2], 16), "address": int(match[3], 16)}
symbols = {}
for line in subprocess.check_output([str(tools / "riscv32-esp-elf-nm"), str(elf)], text=True).splitlines():
    fields = line.split()
    if len(fields) == 3 and fields[2] in ("_stack_start", "_stack_end"):
        symbols[fields[2]] = int(fields[0], 16)
base = sections[".trap"]["address"]
reclaimed = sections[".dram2_uninit"]
report = {
    "ordinary_sram_bytes": reclaimed["address"] - base,
    "reclaimed_sram_bytes": reclaimed["bytes"],
    "rom_reserved_bytes": base + 512 * 1024 - reclaimed["address"] - reclaimed["bytes"],
    "main_stack_bytes": symbols["_stack_start"] - symbols["_stack_end"],
    "controller_heap_capacity_bytes": 80 * 1024,
    "ui_stack_capacity_bytes": 192 * 1024,
    "dma_buffer_bytes": 60 * 1024,
    "note": "Heap/stack capacities are configured budgets; free/peak usage requires hardware measurement.",
}
assert report["reclaimed_sram_bytes"] == 64 * 1024
print(json.dumps(report, indent=2))
