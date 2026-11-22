## Developing on Mac

GPUI compiles Metal shaders at build time using Xcode's Metal Toolchain. To set it up:

1. Ensure Xcode is selected as the active developer directory:

   ```sh
   sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
   ```

2. Download and install the Metal Toolchain component:

   ```sh
   xcodebuild -downloadComponent MetalToolchain
   ```

3. Clear `xcrun`'s lookup cache (if it previously cached the missing-toolchain stub) and verify `metal` works:

   ```sh
   xcrun -k -sdk macosx metal --version
   ```
