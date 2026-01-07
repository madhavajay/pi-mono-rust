"""
pi_mono - Python bindings for pi-mono-rust agent library.

This package provides Python access to the pi-mono-rust agent library,
enabling integration with Anthropic Claude, OpenAI Codex, and other AI providers.

Example usage:
    from pi_mono import AuthStorage, AgentSession

    # Create auth storage
    auth = AuthStorage("/path/to/auth.json")

    # Check if we have credentials
    if auth.has_auth("anthropic"):
        # Create a session
        session = AgentSession(cwd="/path/to/project")

        # Subscribe to events
        def on_event(event):
            print(f"Event: {event}")
        session.subscribe(on_event)

        # Send a prompt
        session.prompt("Hello, world!")
"""

from ._pi_mono import (
    # Classes
    PyAuthStorage as AuthStorage,
    PyAgentSession as AgentSession,

    # OAuth functions
    anthropic_get_auth_url,
    anthropic_exchange_code,
    anthropic_refresh_token,
    openai_codex_get_auth_url,
    openai_codex_exchange_code,
    openai_codex_refresh_token,

    # Utility functions
    get_agent_dir,
)

__all__ = [
    # Classes
    "AuthStorage",
    "AgentSession",

    # OAuth functions
    "anthropic_get_auth_url",
    "anthropic_exchange_code",
    "anthropic_refresh_token",
    "openai_codex_get_auth_url",
    "openai_codex_exchange_code",
    "openai_codex_refresh_token",

    # Utility functions
    "get_agent_dir",
]

__version__ = "0.1.0"
