from tmls.web import server


async def test_health_needs_no_login(aiohttp_client, tmp_path):
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    resp = await client.get("/healthz")
    assert resp.status == 200 and await resp.text() == "ok"
