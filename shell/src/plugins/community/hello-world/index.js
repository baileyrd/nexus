// community-plugins/hello-world/index.js
// Self-contained ES module — no relative imports.
// Drop this folder into ~/.tauri-shell/plugins/ to install.

const plugin = {
  manifest: {
    id: 'community.hello-world',
    name: 'Hello World',
    version: '1.0.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      commands: [
        {
          id: 'hello.greet',
          title: 'Say Hello',
          category: 'Hello World',
        },
      ],
    },
  },

  activate(api) {
    // Greet on load
    api.notifications.show({
      message: '👋 Hello World plugin loaded!',
      type: 'success',
      duration: 3000,
    })

    // Command palette entry
    api.commands.register('hello.greet', () => {
      api.notifications.show({
        message: '🌍 Hello from the community plugin!',
        type: 'info',
        duration: 4000,
      })
    })

    // Status bar item (right side, low priority so it sits at the far right)
    api.statusBar.createItem({
      id: 'hello.status',
      slot: 'right',
      priority: 999,
      text: '👋 Hello',
      tooltip: 'Hello World — click to greet',
      command: 'hello.greet',
    })

    console.info('[community.hello-world] activated')
  },

  deactivate() {
    console.info('[community.hello-world] deactivated')
  },
}

export default plugin
