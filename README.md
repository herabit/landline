# `::landline`

A shitty pipewire-native implementation that seeks to be safer, while interacting well with the Rust async
ecosystem, primarily `::tokio`... Though support for `::smol` may be added later.

When dealing with SPA PODs we're aiming for a zero-copy interface.
