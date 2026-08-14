/*
 * Copyright Daytona Platforms Inc.
 * SPDX-License-Identifier: AGPL-3.0
 */

import { describe, expect, it } from 'vitest'
import { queryKeys } from './queryKeys'

describe('telemetry log query keys', () => {
  it('isolates every cached box telemetry query by organization', () => {
    const params = { from: new Date('2026-08-14T00:00:00.000Z'), to: new Date('2026-08-14T01:00:00.000Z') }

    expect(queryKeys.telemetry.logs('org-1', 'box-1', params)).toEqual([
      'telemetry',
      'org-1',
      'box-1',
      'logs',
      params,
    ])
    expect(queryKeys.telemetry.traces('org-1', 'box-1', params)).toEqual([
      'telemetry',
      'org-1',
      'box-1',
      'traces',
      params,
    ])
    expect(queryKeys.telemetry.metrics('org-1', 'box-1', params)).toEqual([
      'telemetry',
      'org-1',
      'box-1',
      'metrics',
      params,
    ])
    expect(queryKeys.telemetry.traceSpans('org-1', 'box-1', 'trace-1')).toEqual([
      'telemetry',
      'org-1',
      'box-1',
      'traces',
      'trace-1',
    ])
  })
})
