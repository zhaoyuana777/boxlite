/*
 * Copyright 2026 BoxLite AI
 * SPDX-License-Identifier: AGPL-3.0
 */

import { BadRequestException } from '@nestjs/common'
import { MODULE_METADATA, PATH_METADATA } from '@nestjs/common/constants'
import { AdminModule } from './admin.module'
import { AdminBoxesController } from './controllers/boxes.controller'
import { AUDIT_CONTEXT_KEY } from '../audit/decorators/audit.decorator'

describe('AdminModule', () => {
  it('BoxLite admin read exposes the plural admin boxes surface', () => {
    const controllers = Reflect.getMetadata(MODULE_METADATA.CONTROLLERS, AdminModule) as Array<
      new (...args: any[]) => any
    >
    const controllerPaths = controllers.map((controller) => Reflect.getMetadata(PATH_METADATA, controller))

    expect(controllerPaths).toContain('admin/boxes')
  })

  it('BoxLite admin read never audits the raw cursor or an unbounded filter', () => {
    const auditContext = Reflect.getMetadata(AUDIT_CONTEXT_KEY, AdminBoxesController.prototype.findAll)
    const filters = auditContext.requestMetadata.filters({
      query: { cursor: 'opaque-secret', regionId: 'x'.repeat(129), state: 'started' },
    })

    expect(filters).toEqual({
      limit: '50',
      state: 'started',
      organizationId: undefined,
      runnerId: undefined,
      regionId: undefined,
    })
    expect(JSON.stringify(filters)).not.toContain('opaque-secret')
  })

  it('BoxLite admin read rejects an invalid Box ID before querying storage', async () => {
    const service = { findOne: jest.fn() }
    const controller = new AdminBoxesController(service as any)

    await expect(controller.findOne('not-a-box-id')).rejects.toBeInstanceOf(BadRequestException)
    expect(service.findOne).not.toHaveBeenCalled()
  })
})
