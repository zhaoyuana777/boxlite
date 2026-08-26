/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import { BadRequestException, NotFoundException } from '@nestjs/common'
import { BoxDesiredState } from '../../box/enums/box-desired-state.enum'
import { BoxState } from '../../box/enums/box-state.enum'
import { AdminBoxReadService } from './admin-box-read.service'

const row = {
  id: 'Ab3xYz09LmN2',
  name: 'box-name',
  organizationId: '11111111-1111-4111-8111-111111111111',
  runnerId: '22222222-2222-4222-8222-222222222222',
  region: 'us',
  state: BoxState.STARTED,
  desiredState: BoxDesiredState.STARTED,
  cpu: 2,
  mem: 4,
  disk: 20,
  recoverable: false,
  createdAt: new Date('2026-08-25T00:00:00.000Z'),
  updatedAt: new Date('2026-08-26T00:00:00.000Z'),
  cursorUpdatedAt: '2026-08-26T00:00:00.123800Z',
  env: { SECRET: 'must-not-escape' },
  errorReason: 'raw customer-controlled error',
  volumes: [{ mountPath: '/secret' }],
}

function createService() {
  const repository = {
    findAdminPage: jest.fn(),
    findAdminOne: jest.fn(),
  }
  const configService = {
    getOrThrow: jest.fn().mockReturnValue('test-only-cursor-key'),
  }
  return {
    service: new AdminBoxReadService(repository as any, configService as any),
    repository,
  }
}

describe('AdminBoxReadService', () => {
  it('returns only the explicit admin Box allowlist', async () => {
    const { service, repository } = createService()
    repository.findAdminOne.mockResolvedValue(row)

    const result = await service.findOne(row.id)

    expect(result).toEqual({
      id: row.id,
      name: row.name,
      organizationId: row.organizationId,
      runnerId: row.runnerId,
      regionId: row.region,
      state: row.state,
      desiredState: row.desiredState,
      resources: { cpu: 2, memoryGiB: 4, diskGiB: 20 },
      errorCategory: null,
      createdAt: '2026-08-25T00:00:00.000Z',
      updatedAt: '2026-08-26T00:00:00.000Z',
    })
    expect(Object.keys(result).sort()).toEqual(
      [
        'id',
        'name',
        'organizationId',
        'runnerId',
        'regionId',
        'state',
        'desiredState',
        'resources',
        'errorCategory',
        'createdAt',
        'updatedAt',
      ].sort(),
    )
  })

  it('binds an authenticated cursor to its filters and keyset boundary', async () => {
    const { service, repository } = createService()
    repository.findAdminPage.mockResolvedValue({ items: [row], hasMore: true })

    const firstPage = await service.findAll({ limit: 1, state: BoxState.STARTED })
    expect(firstPage.nextCursor).toEqual(expect.any(String))
    if (!firstPage.nextCursor) throw new Error('Expected a next cursor')

    repository.findAdminPage.mockResolvedValue({ items: [], hasMore: false })
    await service.findAll({ limit: 1, state: BoxState.STARTED, cursor: firstPage.nextCursor })

    expect(repository.findAdminPage).toHaveBeenLastCalledWith({
      limit: 1,
      filters: {
        state: BoxState.STARTED,
        organizationId: undefined,
        runnerId: undefined,
        regionId: undefined,
      },
      after: { updatedAt: row.cursorUpdatedAt, id: row.id },
    })
    await expect(
      service.findAll({ limit: 1, state: BoxState.STOPPED, cursor: firstPage.nextCursor }),
    ).rejects.toBeInstanceOf(BadRequestException)
  })

  it('rejects a tampered cursor and reports a missing Box', async () => {
    const { service, repository } = createService()
    repository.findAdminPage.mockResolvedValue({ items: [row], hasMore: true })
    const { nextCursor } = await service.findAll({ limit: 1 })
    if (!nextCursor) throw new Error('Expected a next cursor')
    const last = nextCursor.slice(-1)
    const tampered = `${nextCursor.slice(0, -1)}${last === 'A' ? 'B' : 'A'}`

    await expect(service.findAll({ limit: 1, cursor: tampered })).rejects.toBeInstanceOf(BadRequestException)

    repository.findAdminOne.mockResolvedValue(null)
    await expect(service.findOne(row.id)).rejects.toBeInstanceOf(NotFoundException)
  })
})
