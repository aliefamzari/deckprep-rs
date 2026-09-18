# GPIO/SCART Placeholder

GPIO video output is intentionally inactive during desktop development. The interface
must continue to target a 320x240 logical canvas with an inner safe area so future CRT
output does not require a GUI redesign.

Do not enable a Raspberry Pi DPI overlay or custom modeline until the GPIO-to-SCART
hardware is selected. The implementation will need verified:

- GPIO/DPI pin mapping and voltage levels
- RGB bit depth
- Composite-sync generation and polarity
- SCART blanking and switching voltages
- 240p60 timings at approximately 15.7 kHz horizontal frequency
- Overscan and geometry on the Sony PVM 14M4E

The `pi-gpio-scart.json` profile records this future backend but performs no hardware
configuration.