import asyncio, os, sys
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

async def main():
    p = os.environ["SPIKE"]
    params = StdioServerParameters(command=sys.argv[1], args=["mcp"], env={
        "HYALOS_CONFIG_DIR": f"{p}/c", "HYALOS_DATA_DIR": f"{p}/d", "HYALOS_RUNTIME_DIR": f"{p}/r",
        "HYALOS_AGENT_CREDENTIAL": open(f"{p}/cred").read().strip(), "PATH": os.environ["PATH"]})
    async with stdio_client(params) as (r, w):
        async with ClientSession(r, w) as s:
            try:
                init = await asyncio.wait_for(s.initialize(), 20)
                print("initialize ok:", init.protocolVersion)
                print("tools:", [t.name for t in (await s.list_tools()).tools])
            except Exception as e:
                print("initialize failed:", type(e).__name__, e)
asyncio.run(main())
