# The spatial-os module list. Importing this into a NixOS configuration brings in
# the device contract and all layer modules. Device modules (devices/<codename>)
# import this plus their family.
[
  ../lib/contract # the typed spatial.* device contract
  ./os # common distribution policy (device-independent)
  ./xr # Monado runtime + session wiring
  ./adaptation # per-subsystem backend implementations
]
