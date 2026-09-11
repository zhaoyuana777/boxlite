/*
 * Copyright 2026 BoxLite AI
 * SPDX-License-Identifier: AGPL-3.0
 */

import { RunnerAdapterV0 } from './runnerAdapter.v0'

describe('RunnerAdapterV0 createBox', () => {
  it('passes secrets through to the runner create body', async () => {
    const adapter = new RunnerAdapterV0()
    const create = jest.fn().mockResolvedValue({ data: { daemonVersion: '1.0' } })
    ;(adapter as any).boxApiClient = { create }

    const box = {
      id: 'box-1',
      image: 'base',
      osUser: 'boxlite',
      cpu: 1,
      gpu: 0,
      mem: 1,
      disk: 3,
      env: {},
      secrets: [
        { name: 'openai', value: 'sk-test', hosts: ['api.openai.com'], placeholder: '<BOXLITE_SECRET:openai>' },
      ],
      networkBlockAll: false,
      networkAllowList: undefined,
      authToken: undefined,
      organizationId: undefined,
      region: undefined,
    } as any

    await adapter.createBox(box)

    expect(create).toHaveBeenCalledWith(
      expect.objectContaining({
        secrets: [
          { name: 'openai', value: 'sk-test', hosts: ['api.openai.com'], placeholder: '<BOXLITE_SECRET:openai>' },
        ],
      }),
    )
  })

  it('passes secrets through to the runner recover body', async () => {
    const adapter = new RunnerAdapterV0()
    const recover = jest.fn().mockResolvedValue(undefined)
    ;(adapter as any).boxApiClient = { recover }

    const box = {
      id: 'box-1',
      osUser: 'boxlite',
      cpu: 1,
      gpu: 0,
      mem: 1,
      disk: 3,
      env: {},
      volumes: [],
      secrets: [{ name: 'openai', value: 'sk-test' }],
      networkBlockAll: false,
      networkAllowList: undefined,
      errorReason: 'crashed',
    } as any

    await adapter.recoverBox(box)

    expect(recover).toHaveBeenCalledWith(
      'box-1',
      expect.objectContaining({
        secrets: [{ name: 'openai', value: 'sk-test' }],
      }),
      { 'axios-retry': { retries: 0 } },
    )
  })
})

import axios, { AxiosError } from 'axios'

describe('Recovery transport', () => {
  it('does not replay non-idempotent recovery after a lost response', async () => {
    const create = axios.create.bind(axios)
    let attempts = 0
    const spy = jest.spyOn(axios, 'create').mockImplementation((options) =>
      create({
        ...options,
        adapter: async (config) => {
          attempts++
          // The runner completed the first recovery, but its response was lost.
          if (attempts === 1) throw new AxiosError('response lost', 'ECONNRESET', config)
          return { data: 'Box recovered', status: 200, statusText: 'OK', headers: {}, config }
        },
      }),
    )
    try {
      const adapter = new RunnerAdapterV0()
      await adapter.init({ apiUrl: 'http://runner.invalid', apiKey: 'YOUR_API_KEY' } as any)
      await adapter.recoverBox({ id: 'recovery-test', volumes: [] } as any).catch(() => undefined)
      expect(attempts).toBe(1)
    } finally {
      spy.mockRestore()
    }
  })
})
