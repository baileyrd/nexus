// Shared flag coordinating between `KeyCaptureInput` and
// `KeybindingDispatcher`: while a capture is in progress the global
// dispatcher must NOT consume the chord, otherwise pressing a chord
// that matches an existing binding would fire the command instead of
// being recorded as the new binding.

let active = false;

export function isCapturing(): boolean {
  return active;
}

export function beginCapture(): void {
  active = true;
}

export function endCapture(): void {
  active = false;
}
