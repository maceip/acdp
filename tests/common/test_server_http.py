#!/usr/bin/env python3
"""
HTTP-SSE/Stream Test MCP Server for testing the MCP Proxy

This server provides both SSE and streaming endpoints for MCP testing.
It can be started with either --sse or --stream mode.
"""

import asyncio
import json
import argparse
import random
from datetime import datetime
from typing import Dict, Any
from aiohttp import web
import sys

class TestHttpMCPServer:
    def __init__(self, delay_min: float = 0.1, delay_max: float = 0.5, error_rate: float = 0.05):
        self.delay_min = delay_min
        self.delay_max = delay_max
        self.error_rate = error_rate
        self.request_count = 0
        self.tools = [
            "calculator",
            "file_reader",
            "web_search",
            "database_query",
            "email_sender"
        ]
        self.resources = [
            "config://settings.json",
            "file://documents/readme.md",
            "url://api.example.com/data",
            "database://users/table"
        ]

    def log(self, message: str):
        """Log to stderr"""
        print(f"[{datetime.now().strftime('%H:%M:%S')}] {message}", file=sys.stderr, flush=True)

    async def handle_request(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Handle an incoming JSON-RPC request"""
        self.request_count += 1
        method = request.get("method", "unknown")
        request_id = request.get("id")

        self.log(f"Request #{self.request_count}: {method}")

        # Add random delay
        delay = random.uniform(self.delay_min, self.delay_max)
        await asyncio.sleep(delay)

        # Simulate random errors
        if random.random() < self.error_rate:
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": -32603,
                    "message": f"Simulated error for testing: {method}"
                }
            }

        # Handle different methods
        if method == "initialize":
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "serverInfo": {
                        "name": "test-http-mcp-server",
                        "version": "0.1.0"
                    },
                    "capabilities": {
                        "tools": {"listChanged": True},
                        "resources": {"listChanged": True},
                        "prompts": {"listChanged": True}
                    }
                }
            }

        elif method == "tools/list":
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "tools": [
                        {
                            "name": name,
                            "description": f"Test {name} tool",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "input": {"type": "string"}
                                }
                            }
                        }
                        for name in self.tools
                    ]
                }
            }

        elif method == "tools/call":
            params = request.get("params", {})
            tool_name = params.get("name", "unknown")
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": f"Result from {tool_name}: Success!"
                        }
                    ]
                }
            }

        elif method == "resources/list":
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "resources": [
                        {
                            "uri": uri,
                            "name": uri.split("://")[1],
                            "mimeType": "text/plain"
                        }
                        for uri in self.resources
                    ]
                }
            }

        elif method == "resources/read":
            params = request.get("params", {})
            uri = params.get("uri", "unknown://unknown")
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "text/plain",
                            "text": f"Content of {uri}"
                        }
                    ]
                }
            }

        elif method == "prompts/list":
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "prompts": [
                        {
                            "name": "test_prompt",
                            "description": "A test prompt",
                            "arguments": []
                        }
                    ]
                }
            }

        else:
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": -32601,
                    "message": f"Method not found: {method}"
                }
            }

    async def handle_sse_post(self, request):
        """Handle SSE POST endpoint - send request and get response as SSE"""
        self.log("SSE POST request received")
        response = web.StreamResponse()
        response.headers['Content-Type'] = 'text/event-stream'
        response.headers['Cache-Control'] = 'no-cache'
        response.headers['Connection'] = 'keep-alive'
        await response.prepare(request)

        try:
            # Read request from POST body
            body = await request.text()
            if body:
                rpc_request = json.loads(body)
                result = await self.handle_request(rpc_request)

                # Send as SSE event
                event_data = f"data: {json.dumps(result)}\n\n"
                await response.write(event_data.encode())
        except Exception as e:
            self.log(f"SSE error: {e}")
            error_response = {
                "jsonrpc": "2.0",
                "error": {
                    "code": -32700,
                    "message": f"Parse error: {str(e)}"
                }
            }
            event_data = f"data: {json.dumps(error_response)}\n\n"
            await response.write(event_data.encode())

        return response

    async def handle_sse_get(self, request):
        """Handle SSE GET endpoint - establish persistent SSE connection"""
        self.log("SSE GET connection established (persistent stream)")
        response = web.StreamResponse()
        response.headers['Content-Type'] = 'text/event-stream'
        response.headers['Cache-Control'] = 'no-cache'
        response.headers['Connection'] = 'keep-alive'
        await response.prepare(request)

        try:
            # Send initial connection event
            event_data = f"event: connected\ndata: {{\"type\":\"connected\"}}\n\n"
            await response.write(event_data.encode())

            # Keep connection open and send heartbeat every 30 seconds
            # In a real implementation, this would listen for server-initiated events
            while True:
                await asyncio.sleep(30)
                heartbeat = f": heartbeat\n\n"
                await response.write(heartbeat.encode())
        except asyncio.CancelledError:
            self.log("SSE GET connection closed")
        except Exception as e:
            self.log(f"SSE GET error: {e}")

        return response

    async def handle_stream(self, request):
        """Handle streaming endpoint"""
        self.log("Stream connection established")

        try:
            # Read request from POST body
            body = await request.text()
            rpc_request = json.loads(body)
            result = await self.handle_request(rpc_request)
            return web.json_response(result)
        except Exception as e:
            self.log(f"Stream error: {e}")
            error_response = {
                "jsonrpc": "2.0",
                "error": {
                    "code": -32700,
                    "message": f"Parse error: {str(e)}"
                }
            }
            return web.json_response(error_response)

def main():
    parser = argparse.ArgumentParser(description="HTTP MCP Test Server")
    parser.add_argument("--mode", choices=["sse", "stream"], default="sse", help="Server mode")
    parser.add_argument("--port", type=int, default=8080, help="Port to listen on")
    parser.add_argument("--host", default="127.0.0.1", help="Host to bind to")
    args = parser.parse_args()

    server = TestHttpMCPServer()
    app = web.Application()

    if args.mode == "sse":
        # Support both GET (persistent SSE) and POST (request/response)
        app.router.add_get('/sse', server.handle_sse_get)
        app.router.add_post('/sse', server.handle_sse_post)
        endpoint = f"http://{args.host}:{args.port}/sse"
    else:
        app.router.add_post('/message', server.handle_stream)
        endpoint = f"http://{args.host}:{args.port}/message"

    print(f"Starting HTTP-{args.mode.upper()} MCP test server on {endpoint}", file=sys.stderr, flush=True)
    web.run_app(app, host=args.host, port=args.port, print=lambda x: None)

if __name__ == "__main__":
    main()
