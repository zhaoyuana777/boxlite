/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import { BadRequestException, ValidationPipe } from '@nestjs/common'
import { BoxState } from '../../box/enums/box-state.enum'
import { AdminListBoxesQueryDto } from './admin-list-boxes-query.dto'

const pipe = new ValidationPipe({ transform: true, whitelist: true, forbidNonWhitelisted: true })
const metadata = { type: 'query' as const, metatype: AdminListBoxesQueryDto }

describe('AdminListBoxesQueryDto', () => {
  it('defaults and converts the bounded page size', async () => {
    await expect(pipe.transform({ state: BoxState.STARTED }, metadata)).resolves.toEqual(
      expect.objectContaining({ limit: 50, state: BoxState.STARTED }),
    )
    await expect(pipe.transform({ limit: '200' }, metadata)).resolves.toEqual(expect.objectContaining({ limit: 200 }))
  })

  it.each([
    { limit: '201' },
    { state: 'not-a-state' },
    { organizationId: 'not-a-uuid' },
    { runnerId: 'not-a-uuid' },
    { regionId: '../region' },
    { unknown: 'value' },
  ])('rejects invalid or unknown query input: %j', async (query) => {
    await expect(pipe.transform(query, metadata)).rejects.toBeInstanceOf(BadRequestException)
  })
})
