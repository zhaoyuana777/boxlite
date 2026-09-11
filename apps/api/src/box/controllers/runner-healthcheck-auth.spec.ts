/*
 * Copyright 2026 BoxLite AI
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import { ValidationPipe, type ExecutionContext, type INestApplication } from '@nestjs/common'
import { Test } from '@nestjs/testing'
import type { AddressInfo } from 'net'
import { CombinedAuthGuard } from '../../auth/combined-auth.guard'
import { AuthenticatedRateLimitGuard } from '../../common/guards/authenticated-rate-limit.guard'
import { OrganizationResourceActionGuard } from '../../organization/guards/organization-resource-action.guard'
import { RegionService } from '../../region/services/region.service'
import { SystemRole } from '../../user/enums/system-role.enum'
import { RunnerService } from '../services/runner.service'
import { RunnerController } from './runner.controller'

describe('Runner healthcheck authentication', () => {
  let app: INestApplication
  let endpoint: string
  let authContext: object
  const updateRunnerHealth = jest.fn().mockResolvedValue(undefined)
  const healthcheck = { appVersion: 'test-version' }

  beforeAll(async () => {
    const moduleRef = await Test.createTestingModule({
      controllers: [RunnerController],
      providers: [
        { provide: RunnerService, useValue: { updateRunnerHealth } },
        { provide: RegionService, useValue: {} },
      ],
    })
      .overrideGuard(CombinedAuthGuard)
      .useValue({
        // Start at the authenticated-context boundary; keep RunnerAuthGuard real.
        canActivate: (context: ExecutionContext) => {
          context.switchToHttp().getRequest().user = authContext
          return true
        },
      })
      .overrideGuard(AuthenticatedRateLimitGuard)
      .useValue({ canActivate: () => true })
      .overrideGuard(OrganizationResourceActionGuard)
      .useValue({ canActivate: () => true })
      .compile()

    app = moduleRef.createNestApplication()
    app.setGlobalPrefix('api')
    app.useGlobalPipes(new ValidationPipe({ transform: true }))
    await app.listen(0, '127.0.0.1')
    const address = app.getHttpServer().address() as AddressInfo
    endpoint = `http://127.0.0.1:${address.port}/api/runners/healthcheck`
  })

  beforeEach(() => {
    updateRunnerHealth.mockClear()
  })

  afterAll(async () => {
    await app?.close()
  })

  it.each([
    ['JWT user', { role: SystemRole.USER, userId: 'user-1' }],
    ['user API key', { role: SystemRole.USER, userId: 'user-1', apiKey: { name: 'test-key' } }],
    ['admin', { role: SystemRole.ADMIN, userId: 'admin-1' }],
    ['proxy', { role: 'proxy' }],
  ])('rejects an authenticated %s before updating runner health', async (_name, user) => {
    authContext = user
    const response = await fetch(endpoint, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(healthcheck),
    })

    expect(response.status).toBe(401)
    expect(updateRunnerHealth).not.toHaveBeenCalled()
  })

  it('updates health for the authenticated runner', async () => {
    authContext = { role: 'runner', runnerId: 'runner-1' }
    const response = await fetch(endpoint, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(healthcheck),
    })

    expect(response.status).toBe(201)
    expect(updateRunnerHealth).toHaveBeenCalledTimes(1)
    expect(updateRunnerHealth).toHaveBeenCalledWith(
      'runner-1',
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      healthcheck.appVersion,
    )
  })
})
