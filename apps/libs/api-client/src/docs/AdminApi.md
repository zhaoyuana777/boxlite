# AdminApi

All URIs are relative to *http://localhost:3000*

|Method | HTTP request | Description|
|------------- | ------------- | -------------|
|[**adminCheckInfrastructureLogsAccess**](#admincheckinfrastructurelogsaccess) | **GET** /admin/infrastructure-logs/access | Check infrastructure log access|
|[**adminCreateRunner**](#admincreaterunner) | **POST** /admin/runners | Create runner|
|[**adminDeleteRunner**](#admindeleterunner) | **DELETE** /admin/runners/{id} | Delete runner|
|[**adminGetRunnerById**](#admingetrunnerbyid) | **GET** /admin/runners/{id} | Get runner by ID|
|[**adminListRunners**](#adminlistrunners) | **GET** /admin/runners | List all runners|
|[**adminRecoverBox**](#adminrecoverbox) | **POST** /admin/box/{boxId}/recover | Recover box from error state as an admin|
|[**adminSearchInfrastructureLogs**](#adminsearchinfrastructurelogs) | **GET** /admin/infrastructure-logs | Search infrastructure fallback logs|
|[**adminSearchPlatformLogs**](#adminsearchplatformlogs) | **GET** /admin/infrastructure-logs/platform | Search allowlisted platform OTLP logs|
|[**adminUpdateRunnerScheduling**](#adminupdaterunnerscheduling) | **PATCH** /admin/runners/{id}/scheduling | Update runner scheduling status|

# **adminCheckInfrastructureLogsAccess**
> InfrastructureLogsAccessDto adminCheckInfrastructureLogsAccess()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

const { status, data } = await apiInstance.adminCheckInfrastructureLogsAccess();
```

### Parameters
This endpoint does not have any parameters.


### Return type

**InfrastructureLogsAccessDto**

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**200** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminCreateRunner**
> CreateRunnerResponse adminCreateRunner(adminCreateRunner)


### Example

```typescript
import {
    AdminApi,
    Configuration,
    AdminCreateRunner
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let adminCreateRunner: AdminCreateRunner; //

const { status, data } = await apiInstance.adminCreateRunner(
    adminCreateRunner
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **adminCreateRunner** | **AdminCreateRunner**|  | |


### Return type

**CreateRunnerResponse**

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**201** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminDeleteRunner**
> adminDeleteRunner()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let id: string; //Runner ID (default to undefined)

const { status, data } = await apiInstance.adminDeleteRunner(
    id
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **id** | [**string**] | Runner ID | defaults to undefined|


### Return type

void (empty response body)

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**204** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminGetRunnerById**
> AdminRunner adminGetRunnerById()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let id: string; //Runner ID (default to undefined)

const { status, data } = await apiInstance.adminGetRunnerById(
    id
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **id** | [**string**] | Runner ID | defaults to undefined|


### Return type

**AdminRunner**

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**200** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminListRunners**
> Array<AdminRunner> adminListRunners()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let regionId: string; //Filter runners by region ID (optional) (default to undefined)

const { status, data } = await apiInstance.adminListRunners(
    regionId
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **regionId** | [**string**] | Filter runners by region ID | (optional) defaults to undefined|


### Return type

**Array<AdminRunner>**

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**200** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminRecoverBox**
> Box adminRecoverBox()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let boxId: string; //ID of the box (default to undefined)

const { status, data } = await apiInstance.adminRecoverBox(
    boxId
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **boxId** | [**string**] | ID of the box | defaults to undefined|


### Return type

**Box**

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**200** | Recovery initiated |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminSearchInfrastructureLogs**
> InfrastructureLogs adminSearchInfrastructureLogs()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let from: Date; // (default to undefined)
let to: Date; // (default to undefined)
let source: 'runner' | 'collector'; // (optional) (default to 'runner')
let search: string; //Case-sensitive literal phrase to find in a log message (optional) (default to undefined)
let limit: number; // (optional) (default to 50)
let nextToken: string; //Opaque CloudWatch pagination cursor (optional) (default to undefined)

const { status, data } = await apiInstance.adminSearchInfrastructureLogs(
    from,
    to,
    source,
    search,
    limit,
    nextToken
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **from** | [**Date**] |  | defaults to undefined|
| **to** | [**Date**] |  | defaults to undefined|
| **source** | [**&#39;runner&#39; | &#39;collector&#39;**]**Array<&#39;runner&#39; &#124; &#39;collector&#39; &#124; &#39;11184809&#39;>** |  | (optional) defaults to 'runner'|
| **search** | [**string**] | Case-sensitive literal phrase to find in a log message | (optional) defaults to undefined|
| **limit** | [**number**] |  | (optional) defaults to 50|
| **nextToken** | [**string**] | Opaque CloudWatch pagination cursor | (optional) defaults to undefined|


### Return type

**InfrastructureLogs**

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**200** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminSearchPlatformLogs**
> PaginatedLogs adminSearchPlatformLogs()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let from: Date; //Start of time range (ISO 8601) (default to undefined)
let to: Date; //End of time range (ISO 8601) (default to undefined)
let page: number; //Page number (1-indexed) (optional) (default to 1)
let limit: number; //Number of items per page (optional) (default to 50)
let severities: Array<string>; //Filter by severity levels (DEBUG, INFO, WARN, ERROR) (optional) (default to undefined)
let search: string; //Case-insensitive text search in the log body (optional) (default to undefined)
let source: 'api' | 'worker' | 'runner' | 'box'; // (optional) (default to 'api')
let boxId: string; //Exact Box ID. Required when source is box. (optional) (default to undefined)
let traceId: string; //Exact OpenTelemetry trace ID (optional) (default to undefined)

const { status, data } = await apiInstance.adminSearchPlatformLogs(
    from,
    to,
    page,
    limit,
    severities,
    search,
    source,
    boxId,
    traceId
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **from** | [**Date**] | Start of time range (ISO 8601) | defaults to undefined|
| **to** | [**Date**] | End of time range (ISO 8601) | defaults to undefined|
| **page** | [**number**] | Page number (1-indexed) | (optional) defaults to 1|
| **limit** | [**number**] | Number of items per page | (optional) defaults to 50|
| **severities** | **Array&lt;string&gt;** | Filter by severity levels (DEBUG, INFO, WARN, ERROR) | (optional) defaults to undefined|
| **search** | [**string**] | Case-insensitive text search in the log body | (optional) defaults to undefined|
| **source** | [**&#39;api&#39; | &#39;worker&#39; | &#39;runner&#39; | &#39;box&#39;**]**Array<&#39;api&#39; &#124; &#39;worker&#39; &#124; &#39;runner&#39; &#124; &#39;box&#39; &#124; &#39;11184809&#39;>** |  | (optional) defaults to 'api'|
| **boxId** | [**string**] | Exact Box ID. Required when source is box. | (optional) defaults to undefined|
| **traceId** | [**string**] | Exact OpenTelemetry trace ID | (optional) defaults to undefined|


### Return type

**PaginatedLogs**

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**200** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **adminUpdateRunnerScheduling**
> adminUpdateRunnerScheduling()


### Example

```typescript
import {
    AdminApi,
    Configuration
} from './api';

const configuration = new Configuration();
const apiInstance = new AdminApi(configuration);

let id: string; // (default to undefined)

const { status, data } = await apiInstance.adminUpdateRunnerScheduling(
    id
);
```

### Parameters

|Name | Type | Description  | Notes|
|------------- | ------------- | ------------- | -------------|
| **id** | [**string**] |  | defaults to undefined|


### Return type

void (empty response body)

### Authorization

[bearer](../README.md#bearer), [oauth2](../README.md#oauth2)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
|**204** |  |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

