"""
MiMo-VL Vision MCP Server — provides vision capabilities to Claude Code.
Uses Xiaomi MiMo-VL 2.5 via Anthropic-compatible API.

Tools:
  - describe_image: Describe a single image (local file or URL)
  - compare_images: Describe differences between two images
"""

import base64
import ipaddress
import mimetypes
import os
import socket
import sys
import urllib.parse
from pathlib import Path
from typing import Any

from anthropic import Anthropic
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import Tool, TextContent

# Load .env file from the same directory as this script (fallback for when
# MIMO_API_KEY is not already in the environment — e.g. Claude Code MCP launch).
_ENV_FILE = Path(__file__).resolve().parent / ".env"
if _ENV_FILE.is_file():
    with open(_ENV_FILE) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, val = line.partition("=")
            key, val = key.strip(), val.strip()
            if key and key not in os.environ:
                os.environ[key] = val

API_KEY = os.environ.get("MIMO_API_KEY", "")
BASE_URL = "https://api.xiaomimimo.com/anthropic"
MODEL = "mimo-v2.5"

# Security: only allow image reads under these directories + known image extensions.
ALLOWED_ROOTS = [
    Path.cwd(),
    Path.cwd() / ".playwright-mcp",
]
ALLOWED_EXTENSIONS = frozenset({".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"})

# SSRF: block private / loopback / link-local IP ranges.
_BLOCKED_NETS = [
    ipaddress.ip_network("10.0.0.0/8"),
    ipaddress.ip_network("172.16.0.0/12"),
    ipaddress.ip_network("192.168.0.0/16"),
    ipaddress.ip_network("127.0.0.0/8"),
    ipaddress.ip_network("169.254.0.0/16"),
    ipaddress.ip_network("::1/128"),
    ipaddress.ip_network("fc00::/7"),
    ipaddress.ip_network("fe80::/10"),
]

server = Server("mimo-vision")


def _get_client() -> Anthropic:
    return Anthropic(api_key=API_KEY, base_url=BASE_URL)


def _guess_media_type(path: str) -> str:
    mt, _ = mimetypes.guess_type(path)
    return mt or "image/png"


def _is_ip_allowed(host: str) -> bool:
    """Reject private / loopback / link-local addresses to prevent SSRF."""
    try:
        addrs = socket.getaddrinfo(host, None)
        for entry in addrs:
            ip_str = entry[4][0]
            ip = ipaddress.ip_address(ip_str)
            for net in _BLOCKED_NETS:
                if ip in net:
                    return False
        return True
    except socket.gaierror:
        return False


def _validate_url(url: str) -> str:
    """Validate a URL: HTTPS only, non-local host, no weird schemes."""
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme not in ("https",):
        raise ValueError(f"Only HTTPS URLs are allowed, got: {parsed.scheme}")
    if not parsed.hostname:
        raise ValueError("URL has no hostname")
    if not _is_ip_allowed(parsed.hostname):
        raise ValueError(f"Access to internal/private host denied: {parsed.hostname}")
    return url


def _read_local_image(path: str) -> dict:
    """Read a local image and return as an Anthropic base64 source block.

    Security: resolves to canonical path, verifies it's under an allowed root
    directory, and validates the extension is a known image type.
    """
    p = Path(path).resolve()
    if not p.exists():
        raise FileNotFoundError(f"Image not found: {path}")
    suffix = p.suffix.lower()
    if suffix not in ALLOWED_EXTENSIONS:
        raise ValueError(
            f"File type '{suffix}' is not an allowed image format. "
            f"Allowed: {', '.join(sorted(ALLOWED_EXTENSIONS))}"
        )
    # Require the file is under one of the allowed root directories.
    if not any(p.is_relative_to(root) for root in ALLOWED_ROOTS):
        raise PermissionError(
            f"Image path is outside allowed directories. "
            f"Allowed roots: {', '.join(str(r) for r in ALLOWED_ROOTS)}"
        )
    data = p.read_bytes()
    encoded = base64.standard_b64encode(data).decode("ascii")
    media_type = _guess_media_type(str(p))
    return {
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": encoded,
        },
    }


def _build_image_content(image_path: str) -> dict:
    """Build an image content block from a path or URL.

    For URLs: validates HTTPS scheme + non-local host, fetches the image
    with redirect-following disabled (each redirect hop must be re-validated
    to prevent SSRF bypass), and re-encodes as base64.
    For local paths: resolves canonical path, validates extension + root scope.
    """
    if image_path.startswith(("http://", "https://")):
        url = _validate_url(image_path)
        import urllib.request

        # Disable automatic redirect following — prevents URL→IP re-bind
        # bypass where a valid external URL 302's to an internal address.
        class _NoRedirectHandler(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, newurl):
                return None

        opener = urllib.request.build_opener(_NoRedirectHandler)
        with opener.open(url, timeout=15) as resp:
            content_type = resp.headers.get("Content-Type", "")
            if not content_type.startswith("image/"):
                raise ValueError(
                    f"URL did not return an image. Content-Type: {content_type}"
                )
            data = resp.read()
        encoded = base64.standard_b64encode(data).decode("ascii")
        media_type = content_type.split(";")[0].strip() or _guess_media_type(url)
        return {
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": encoded,
            },
        }
    return _read_local_image(image_path)


async def _call_mimo(messages: list[dict], max_tokens: int = 2048) -> str:
    client = _get_client()
    response = client.messages.create(
        model=MODEL,
        max_tokens=max_tokens,
        system="You are MiMo, an AI assistant developed by Xiaomi. You are a visual analysis expert. Describe images with precision — note all UI elements, text content, colors, layout structure, and design patterns.",
        messages=messages,
        stream=False,
        thinking={"type": "disabled"},
    )
    return response.content[0].text


@server.list_tools()
async def list_tools() -> list[Tool]:
    return [
        Tool(
            name="describe_image",
            description="Analyze a single image using MiMo-VL vision model. "
            "Returns a detailed description of the image content — UI elements, "
            "text, colors, layout, and design patterns. "
            "Accepts a local file path or a URL.",
            inputSchema={
                "type": "object",
                "properties": {
                    "image_path": {
                        "type": "string",
                        "description": "Path to a local image file (e.g. screenshot.png) or an HTTP(S) URL.",
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Optional custom prompt. Default: describe image in detail with focus on UI elements.",
                    },
                },
                "required": ["image_path"],
            },
        ),
        Tool(
            name="compare_images",
            description="Compare two images side-by-side. Describe differences in UI, "
            "layout, colors, text, and visual design. Useful for before/after "
            "comparison of UI changes. Accepts local file paths or URLs.",
            inputSchema={
                "type": "object",
                "properties": {
                    "image_a": {
                        "type": "string",
                        "description": "First image (local path or URL) — typically 'before'.",
                    },
                    "image_b": {
                        "type": "string",
                        "description": "Second image (local path or URL) — typically 'after'.",
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Optional custom comparison prompt.",
                    },
                },
                "required": ["image_a", "image_b"],
            },
        ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict[str, Any]) -> list[TextContent]:
    try:
        if name == "describe_image":
            image_path = arguments["image_path"]
            prompt = arguments.get(
                "prompt",
                "Please describe this image in rich detail. Pay close attention to: "
                "1) UI elements and their positions, 2) All visible text content, "
                "3) Colors and visual style, 4) Layout structure and spacing, "
                "5) Any design patterns or notable visual characteristics.",
            )
            content = _build_image_content(image_path)
            messages = [
                {
                    "role": "user",
                    "content": [content, {"type": "text", "text": prompt}],
                }
            ]
            result = await _call_mimo(messages)
            return [TextContent(type="text", text=result)]

        elif name == "compare_images":
            image_a = arguments["image_a"]
            image_b = arguments["image_b"]
            prompt = arguments.get(
                "prompt",
                "Compare these two images carefully. Describe all differences in: "
                "1) UI layout and element positions, 2) Colors and visual styling, "
                "3) Text content changes, 4) Spacing and sizing differences, "
                "5) Any added, removed, or modified elements. "
                "Be specific — reference positions (top-left, center, etc.) and "
                "quantify changes when possible.",
            )
            content_a = _build_image_content(image_a)
            content_b = _build_image_content(image_b)
            messages = [
                {
                    "role": "user",
                    "content": [
                        content_a,
                        content_b,
                        {"type": "text", "text": prompt},
                    ],
                }
            ]
            result = await _call_mimo(messages, max_tokens=3072)
            return [TextContent(type="text", text=result)]

        else:
            return [TextContent(type="text", text=f"Unknown tool: {name}")]

    except Exception as e:
        return [TextContent(type="text", text=f"Vision error: {e}")]


async def main():
    if not API_KEY:
        print(
            "MIMO_API_KEY environment variable is not set. "
            "Please set it before running this server.",
            file=sys.stderr,
        )
        sys.exit(1)
    async with stdio_server() as (read_stream, write_stream):
        await server.run(read_stream, write_stream, server.create_initialization_options())


if __name__ == "__main__":
    import asyncio
    asyncio.run(main())
