// @vitest-environment jsdom
/*
 * Copyright Daytona Platforms Inc.
 * SPDX-License-Identifier: AGPL-3.0
 */

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { useInfrastructureLogs, useInfrastructureLogsAccess, usePlatformLogs } from './useInfrastructureLogs'

const mocks = vi.hoisted(() => ({
  access: vi.fn(),
  infrastructure: vi.fn(),
  platform: vi.fn(),
}))

vi.mock('@/hooks/useApi', () => ({
  useApi: () => ({
    adminApi: {
      adminCheckInfrastructureLogsAccess: mocks.access,
      adminSearchInfrastructureLogs: mocks.infrastructure,
      adminSearchPlatformLogs: mocks.platform,
    },
  }),
}))

function QueryProbe() {
  useInfrastructureLogsAccess()
  useInfrastructureLogs({
    source: 'runner',
    from: new Date('2026-08-14T00:00:00.000Z'),
    to: new Date('2026-08-14T01:00:00.000Z'),
    search: 'failure',
    limit: 25,
    nextToken: 'cursor-2',
  })
  usePlatformLogs({
    source: 'api',
    from: new Date('2026-08-14T00:00:00.000Z'),
    to: new Date('2026-08-14T01:00:00.000Z'),
    page: 2,
    limit: 25,
    traceId: '0123456789abcdef0123456789abcdef',
  })
  return null
}

describe('infrastructure log queries', () => {
  let root: Root | null = null
  let queryClient: QueryClient

  beforeAll(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true
  })

  beforeEach(() => {
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    mocks.access.mockResolvedValue({ data: { canRead: true } })
    mocks.infrastructure.mockResolvedValue({ data: { items: [] } })
    mocks.platform.mockResolvedValue({ data: { items: [], total: 0, page: 2, totalPages: 0 } })
  })

  afterEach(() => {
    act(() => root?.unmount())
    root = null
    queryClient.clear()
    document.body.innerHTML = ''
    vi.clearAllMocks()
  })

  it('uses the generated admin client for every infrastructure log request', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    root = createRoot(host)

    await act(async () => {
      root?.render(
        <QueryClientProvider client={queryClient}>
          <QueryProbe />
        </QueryClientProvider>,
      )
      await Promise.resolve()
    })

    expect(mocks.access).toHaveBeenCalledWith({ timeout: 10_000 })
    expect(mocks.infrastructure).toHaveBeenCalledWith(
      new Date('2026-08-14T00:00:00.000Z'),
      new Date('2026-08-14T01:00:00.000Z'),
      'runner',
      'failure',
      25,
      'cursor-2',
      { timeout: 10_000 },
    )
    expect(mocks.platform).toHaveBeenCalledWith(
      new Date('2026-08-14T00:00:00.000Z'),
      new Date('2026-08-14T01:00:00.000Z'),
      2,
      25,
      undefined,
      undefined,
      'api',
      undefined,
      '0123456789abcdef0123456789abcdef',
      { timeout: 10_000 },
    )
  })
})
