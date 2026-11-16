import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import test from 'node:test'

import { ClusterApi, ClusterApiError } from '../src/api.js'

async function startStub() {
  const state = { logins: 0, validToken: 'token-1' }
  const requests = []
  const server = createServer(async (request, response) => {
    let body = ''
    for await (const chunk of request) body += chunk
    requests.push({ method: request.method, url: request.url, authorization: request.headers.authorization })

    if (request.url === '/api/auth/login' && request.method === 'POST') {
      const parsed = JSON.parse(body)
      if (parsed.username !== 'admin' || parsed.password !== 'secret') {
        response.writeHead(401, { 'content-type': 'application/json' })
        response.end(JSON.stringify({ code: 'unauthorized', message: 'bad credentials' }))
        return
      }
      state.logins += 1
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ token: state.validToken, token_type: 'Bearer', expires_at: 0 }))
      return
    }

    if (request.headers.authorization !== `Bearer ${state.validToken}`) {
      response.writeHead(401, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ code: 'unauthorized', message: 'expired token' }))
      return
    }

    if (request.url === '/api/v1/servers') {
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify([{ id: 'srv-1', name: 'survival' }]))
      return
    }

    if (request.url === '/api/v1/servers/missing') {
      response.writeHead(404, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ code: 'not_found', message: 'server not found' }))
      return
    }

    response.writeHead(204)
    response.end()
  })

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()
  return {
    state,
    requests,
    url: `http://127.0.0.1:${port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

test('logs in lazily, caches the token and maps API errors', async () => {
  const stub = await startStub()
  try {
    const api = new ClusterApi({
      apiUrl: stub.url,
      username: 'admin',
      password: 'secret',
      timeoutMs: 5000,
    })

    const servers = await api.listServers()
    assert.deepEqual(servers, [{ id: 'srv-1', name: 'survival' }])
    assert.equal(stub.state.logins, 1)
    await api.listServers()
    assert.equal(stub.state.logins, 1, 'token should be reused')

    await assert.rejects(
      () => api.getServer('missing'),
      (error) => {
        assert.ok(error instanceof ClusterApiError)
        assert.equal(error.status, 404)
        assert.equal(error.code, 'not_found')
        assert.equal(error.message, 'server not found')
        return true
      },
    )

    const deleteResult = await api.deleteSchedule('srv-1', 'schedule-1')
    assert.equal(deleteResult, null)
  } finally {
    await stub.close()
  }
})

test('re-authenticates once when the token expires', async () => {
  const stub = await startStub()
  try {
    const api = new ClusterApi({
      apiUrl: stub.url,
      apiToken: 'stale-token',
      username: 'admin',
      password: 'secret',
      timeoutMs: 5000,
    })
    stub.state.validToken = 'token-2'
    const servers = await api.listServers()
    assert.deepEqual(servers, [{ id: 'srv-1', name: 'survival' }])
    assert.equal(stub.state.logins, 1)
  } finally {
    await stub.close()
  }
})

test('reports a network failure without leaking internals', async () => {
  const api = new ClusterApi({
    apiUrl: 'http://127.0.0.1:9',
    apiToken: 'token',
    timeoutMs: 1000,
  })
  await assert.rejects(
    () => api.listServers(),
    (error) => {
      assert.ok(error instanceof ClusterApiError)
      assert.equal(error.code, 'network_error')
      assert.match(error.message, /Could not reach the control plane/)
      return true
    },
  )
})
