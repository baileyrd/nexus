// shell/src/types/sandboxRuntime.d.ts
//
// Ambient declaration for the virtual module emitted by
// `shell/vite.sandbox-runtime-plugin.ts`. The default export is the
// bundled `bootstrapSandboxedPlugin` runtime as an ESM source string;
// `shell/src/main.tsx` blob-wraps it on first sandbox load so the
// iframe srcdoc can dynamic-import a self-contained module without a
// bare-specifier resolver. See F-8.1.1-fo1.

declare module 'virtual:sandbox-runtime' {
  const source: string
  export default source
}
