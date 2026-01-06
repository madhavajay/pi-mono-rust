const path = require("node:path");
const readline = require("node:readline");
const { createRequire } = require("node:module");
const { pathToFileURL } = require("node:url");

function createNoOpUI() {
  return {
    select: async () => undefined,
    confirm: async () => false,
    input: async () => undefined,
    notify: () => {},
    setStatus: () => {},
    setWidget: () => {},
    setTitle: () => {},
    custom: async () => undefined,
    setEditorText: () => {},
    getEditorText: () => "",
    editor: async () => undefined,
    get theme() {
      return {};
    },
  };
}

function createAbortSignal() {
  return {
    aborted: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  };
}

function createExtensionApi(extensionPath, registry) {
  const handlers = registry.handlers;
  return {
    on(event, handler) {
      if (!handlers[event]) {
        handlers[event] = [];
      }
      handlers[event].push(handler);
    },
    registerTool(tool) {
      if (tool && tool.name) {
        registry.tools.push({
          name: tool.name,
          label: tool.label,
          description: tool.description,
          parameters: tool.parameters,
        });
        registry.toolHandlers[tool.name] = tool;
      }
    },
    registerCommand(name, options) {
      if (!name) return;
      registry.commands.push({
        name,
        description: options && options.description ? options.description : undefined,
      });
    },
    registerShortcut(shortcut, options) {
      if (!shortcut) return;
      registry.shortcuts.push({
        shortcut,
        description: options && options.description ? options.description : undefined,
      });
    },
    registerFlag(name, options) {
      if (!name || !options) return;
      registry.flags.push({
        name,
        description: options.description,
        type: options.type,
        default: options.default,
      });
      if (options.default !== undefined && registry.flagValues[name] === undefined) {
        registry.flagValues[name] = options.default;
      }
    },
    getFlag(name) {
      return registry.flagValues[name];
    },
    registerMessageRenderer(customType) {
      if (!customType) return;
      registry.messageRenderers.push({ customType });
    },
    sendMessage: () => {},
    appendEntry: () => {},
    exec: async () => {
      throw new Error("exec is not supported in the Rust extension host yet");
    },
    getActiveTools: () => [],
    getAllTools: () => [],
    setActiveTools: () => {},
    events: {},
  };
}

function createJitiLoader(extensionPath) {
  const tryCreateJiti = (requireFn) => {
    try {
      const { createJiti } = requireFn("jiti");
      return createJiti;
    } catch (err) {
      return null;
    }
  };

  const requireFromExtension = createRequire(path.resolve(extensionPath));
  let createJiti = tryCreateJiti(requireFromExtension);
  if (!createJiti) {
    const requireFromCwd = createRequire(path.resolve(process.cwd(), "index.js"));
    createJiti = tryCreateJiti(requireFromCwd);
  }

  if (!createJiti) {
    return null;
  }

  return createJiti(extensionPath, { interopDefault: true, cache: false });
}

async function loadModule(extensionPath) {
  const ext = path.extname(extensionPath).toLowerCase();
  if (ext === ".ts" || ext === ".tsx") {
    const jiti = createJitiLoader(extensionPath);
    if (!jiti) {
      throw new Error(
        "TypeScript extensions require the 'jiti' package. Install it in your project or compile the extension to JavaScript.",
      );
    }
    return jiti(extensionPath);
  }

  try {
    return require(extensionPath);
  } catch (err) {
    return import(pathToFileURL(extensionPath).href);
  }
}

async function loadExtension(extensionPath) {
  const registry = {
    path: extensionPath,
    handlers: {},
    tools: [],
    toolHandlers: {},
    commands: [],
    flags: [],
    flagValues: {},
    shortcuts: [],
    messageRenderers: [],
  };

  const mod = await loadModule(extensionPath);

  const factory = mod && (mod.default || mod);
  if (typeof factory !== "function") {
    throw new Error(`Extension ${extensionPath} does not export a function`);
  }

  const api = createExtensionApi(extensionPath, registry);
  factory(api);
  return registry;
}

function createContext(payload) {
  const data = payload || {};
  const sessionEntries = Array.isArray(data.sessionEntries) ? data.sessionEntries : [];
  const model = data.model;
  return {
    ui: createNoOpUI(),
    hasUI: Boolean(data.hasUI),
    cwd: data.cwd || process.cwd(),
    sessionManager: {
      getEntries() {
        return sessionEntries;
      },
    },
    modelRegistry: {
      getApiKey() {
        return undefined;
      },
    },
    model,
    isIdle() {
      return Boolean(data.isIdle);
    },
    abort() {},
    hasPendingMessages() {
      return Boolean(data.hasPendingMessages);
    },
  };
}

async function emitEvent(extensions, event, context) {
  let result;
  const errors = [];
  const ctx = createContext(context);

  for (const ext of extensions) {
    const handlers = ext.handlers[event.type] || [];
    if (!handlers.length) continue;

    for (const handler of handlers) {
      try {
        const handlerResult = await handler(event, ctx);
        if (event.type === "session_before_compact" && handlerResult) {
          result = handlerResult;
          if (result.cancel) {
            return { result, errors };
          }
        }
        if (event.type === "tool_call" && handlerResult) {
          result = handlerResult;
          if (result.block) {
            return { result, errors };
          }
        }
        if (event.type === "tool_result" && handlerResult) {
          result = handlerResult;
        }
      } catch (err) {
        errors.push({
          extensionPath: ext.path,
          event: event.type,
          error: err && err.message ? err.message : String(err),
        });
      }
    }
  }

  return { result, errors };
}

function sanitizeExtensions(extensions) {
  return extensions.map((ext) => ({
    path: ext.path,
    tools: ext.tools,
    commands: ext.commands,
    flags: ext.flags,
    shortcuts: ext.shortcuts,
    messageRenderers: ext.messageRenderers,
    handlerCounts: Object.fromEntries(
      Object.entries(ext.handlers).map(([key, value]) => [key, value.length]),
    ),
  }));
}

function applyFlags(extensions, flags) {
  const entries = flags && typeof flags === "object" ? Object.entries(flags) : [];
  if (entries.length === 0) return;
  for (const ext of extensions) {
    const names = new Set((ext.flags || []).map((flag) => flag.name));
    if (names.size === 0) continue;
    for (const [name, value] of entries) {
      if (names.has(name)) {
        ext.flagValues[name] = value;
      }
    }
  }
}

async function handleMessage(message, state) {
  if (message.type === "init") {
    const extensions = [];
    const errors = [];
    for (const extPath of message.extensions || []) {
      try {
        const resolved = path.resolve(extPath);
        const ext = await loadExtension(resolved);
        extensions.push(ext);
      } catch (err) {
        errors.push({
          extensionPath: extPath,
          error: err && err.message ? err.message : String(err),
        });
      }
    }
    state.extensions = extensions;
    state.toolHandlers = Object.assign(
      {},
      ...extensions.map((ext) => ext.toolHandlers || {}),
    );
    return {
      ok: true,
      extensions: sanitizeExtensions(extensions),
      errors,
    };
  }

  if (message.type === "set_flags") {
    applyFlags(state.extensions, message.flags);
    return { ok: true };
  }

  if (message.type === "invoke_tool") {
    const tool = state.toolHandlers[message.name];
    if (!tool || typeof tool.execute !== "function") {
      return { ok: false, error: `Tool ${message.name} not found` };
    }
    try {
      const ctx = createContext(message.context);
      const result = await tool.execute(
        message.toolCallId,
        message.input ?? {},
        undefined,
        ctx,
        createAbortSignal(),
      );
      return { ok: true, result: result ?? null };
    } catch (err) {
      return {
        ok: false,
        error: err && err.message ? err.message : String(err),
      };
    }
  }

  if (message.type === "emit") {
    const { result, errors } = await emitEvent(
      state.extensions,
      message.event,
      message.context,
    );
    return {
      ok: true,
      result: result === undefined ? null : result,
      errors,
    };
  }

  return { ok: false, error: "Unknown message type" };
}

async function main() {
  const state = { extensions: [], toolHandlers: {} };
  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });

  for await (const line of rl) {
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch (err) {
      process.stdout.write(
        JSON.stringify({
          id: null,
          ok: false,
          error: "Invalid JSON",
        }) + "\n",
      );
      continue;
    }

    const response = await handleMessage(message, state);
    process.stdout.write(
      JSON.stringify({
        id: message.id ?? null,
        ...response,
      }) + "\n",
    );
  }
}

main().catch((err) => {
  process.stderr.write(String(err) + "\n");
  process.exit(1);
});
