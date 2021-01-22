@echo off

cargo run || goto error

vmrun -T ws reset "D:\VMs\flugzeug_os\flugzeug_os.vmx"

:error