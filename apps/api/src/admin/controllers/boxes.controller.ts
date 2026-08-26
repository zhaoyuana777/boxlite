/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import {
  BadRequestException,
  Controller,
  Get,
  Header,
  HttpCode,
  Param,
  Query,
  UseGuards,
  ValidationPipe,
} from '@nestjs/common'
import { ApiBearerAuth, ApiOAuth2, ApiOperation, ApiParam, ApiResponse, ApiTags } from '@nestjs/swagger'
import { Audit } from '../../audit/decorators/audit.decorator'
import { AuditAction } from '../../audit/enums/audit-action.enum'
import { AuditTarget } from '../../audit/enums/audit-target.enum'
import { CombinedAuthGuard } from '../../auth/combined-auth.guard'
import { SystemActionGuard } from '../../auth/system-action.guard'
import { RequiredApiRole } from '../../common/decorators/required-role.decorator'
import { safeAuditFilter } from '../../common/constants/backoffice.constants'
import { isBoxId } from '../../box/utils/box-id.util'
import { SystemRole } from '../../user/enums/system-role.enum'
import { AdminBoxStatusDto } from '../dto/admin-box.dto'
import { AdminListBoxesQueryDto } from '../dto/admin-list-boxes-query.dto'
import { AdminPaginatedBoxesDto } from '../dto/admin-paginated-boxes.dto'
import { AdminBoxReadService } from '../services/admin-box-read.service'

const STRICT_QUERY_PIPE = new ValidationPipe({ transform: true, whitelist: true, forbidNonWhitelisted: true })

@ApiTags('admin')
@Controller('admin/boxes')
@UseGuards(CombinedAuthGuard, SystemActionGuard)
@RequiredApiRole([SystemRole.ADMIN])
@ApiOAuth2(['openid', 'profile', 'email'])
@ApiBearerAuth()
export class AdminBoxesController {
  constructor(private readonly adminBoxReadService: AdminBoxReadService) {}

  @Get(':boxId')
  @HttpCode(200)
  @Header('Cache-Control', 'private, no-store')
  @ApiOperation({ summary: 'Get Box status across organizations', operationId: 'adminGetBoxById' })
  @ApiParam({ name: 'boxId', description: '12-character Box ID', type: String })
  @ApiResponse({ status: 200, type: AdminBoxStatusDto })
  @ApiResponse({ status: 400, description: 'Invalid Box ID' })
  @ApiResponse({ status: 404, description: 'Box not found' })
  @Audit({
    action: AuditAction.READ,
    targetType: AuditTarget.BOX,
    targetIdFromRequest: (req) => req.params.boxId,
    requestMetadata: { operation: () => 'adminGetBoxById' },
  })
  async findOne(@Param('boxId') boxId: string): Promise<AdminBoxStatusDto> {
    if (!isBoxId(boxId)) {
      throw new BadRequestException('Invalid Box ID')
    }
    return this.adminBoxReadService.findOne(boxId)
  }

  @Get()
  @HttpCode(200)
  @Header('Cache-Control', 'private, no-store')
  @ApiOperation({ summary: 'List Box status across organizations', operationId: 'adminListBoxes' })
  @ApiResponse({ status: 200, type: AdminPaginatedBoxesDto })
  @ApiResponse({ status: 400, description: 'Invalid query or cursor' })
  @Audit({
    action: AuditAction.READ,
    targetType: AuditTarget.BOX,
    requestMetadata: {
      operation: () => 'adminListBoxes',
      filters: (req) => ({
        limit: safeAuditFilter(req.query.limit) ?? '50',
        state: safeAuditFilter(req.query.state),
        organizationId: safeAuditFilter(req.query.organizationId),
        runnerId: safeAuditFilter(req.query.runnerId),
        regionId: safeAuditFilter(req.query.regionId),
      }),
    },
  })
  async findAll(@Query(STRICT_QUERY_PIPE) query: AdminListBoxesQueryDto): Promise<AdminPaginatedBoxesDto> {
    return this.adminBoxReadService.findAll(query)
  }
}
