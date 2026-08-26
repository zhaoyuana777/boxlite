/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import { ApiPropertyOptional } from '@nestjs/swagger'
import { Type } from 'class-transformer'
import { IsEnum, IsInt, IsOptional, IsString, IsUUID, Matches, Max, MaxLength, Min } from 'class-validator'
import { BoxState } from '../../box/enums/box-state.enum'

export class AdminListBoxesQueryDto {
  @ApiPropertyOptional({ description: 'Opaque cursor returned by the previous page', maxLength: 4096 })
  @IsOptional()
  @IsString()
  @MaxLength(4096)
  cursor?: string

  @ApiPropertyOptional({ default: 50, minimum: 1, maximum: 200, type: Number })
  @Type(() => Number)
  @IsInt()
  @Min(1)
  @Max(200)
  limit = 50

  @ApiPropertyOptional({ enum: BoxState, enumName: 'BoxState' })
  @IsOptional()
  @IsEnum(BoxState)
  state?: BoxState

  @ApiPropertyOptional({ format: 'uuid' })
  @IsOptional()
  @IsUUID()
  organizationId?: string

  @ApiPropertyOptional({ format: 'uuid' })
  @IsOptional()
  @IsUUID()
  runnerId?: string

  @ApiPropertyOptional({ description: 'BoxLite Region ID', maxLength: 128 })
  @IsOptional()
  @IsString()
  @Matches(/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/)
  regionId?: string
}
