#/bin/sh
sudo tunctl -t guest_net -u $USER
sudo ip link set up guest_net
