// Hello JS — demo script plugin for Nexus.
//
// Exports the standard plugin contract:
//   dispatch(handlerId, args, ctx) → result
//   onInit(ctx)                    → void (optional lifecycle hook)

export function dispatch(handlerId, args, ctx) {
  switch (handlerId) {
    case 1:
      return { content: "Hello from a JS plugin!" };
    case 2:
      // Demonstrate async settings access via the ctx.
      return ctx.settings.get().then((settings) => ({
        content: `Greetings from JS! Settings: ${JSON.stringify(settings)}`,
      }));
    default:
      return { error: `Unknown handler: ${handlerId}` };
  }
}

export function onInit(ctx) {
  console.log(`[${ctx.pluginId}] JS plugin initialized`);
}
