/*
 * Copyright Daytona Platforms Inc.
 * SPDX-License-Identifier: AGPL-3.0
 */

import { validate } from 'class-validator'
import { InfrastructureLogsQueryDto } from './infrastructure-logs.dto'

describe('InfrastructureLogsQueryDto', () => {
  it('rejects calendar-invalid date bounds', async () => {
    const query = new InfrastructureLogsQueryDto()
    query.from = '2026-02-30T00:00:00.000Z'
    query.to = '2026-03-01T00:00:00.000Z'

    const errors = await validate(query)

    expect(errors).toEqual(expect.arrayContaining([expect.objectContaining({ property: 'from' })]))
  })
})
