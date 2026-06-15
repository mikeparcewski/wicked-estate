// sample.bicep — integration corpus fixture for the Bicep extractor
// Demonstrates params, vars, resources, and outputs.

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------
@description('Azure region for all resources')
param location string = resourceGroup().location

@description('Deployment environment (dev / staging / prod)')
@allowed(['dev', 'staging', 'prod'])
param environment string = 'dev'

@description('Name prefix applied to every resource')
param projectName string = 'wicked-estate'

@minLength(8)
@maxLength(32)
@secure()
param adminPassword string

// ---------------------------------------------------------------------------
// Variables
// ---------------------------------------------------------------------------
var namePrefix = '${projectName}-${environment}'
var commonTags = {
  Project: projectName
  Environment: environment
  ManagedBy: 'bicep'
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------
resource storageAccount 'Microsoft.Storage/storageAccounts@2023-01-01' = {
  name: '${replace(namePrefix, '-', '')}sa'
  location: location
  kind: 'StorageV2'
  sku: {
    name: 'Standard_LRS'
  }
  properties: {
    supportsHttpsTrafficOnly: true
    minimumTlsVersion: 'TLS1_2'
    allowBlobPublicAccess: false
  }
  tags: commonTags
}

resource appServicePlan 'Microsoft.Web/serverfarms@2023-01-01' = {
  name: '${namePrefix}-plan'
  location: location
  kind: 'linux'
  sku: {
    name: 'B1'
    tier: 'Basic'
  }
  properties: {
    reserved: true
  }
  tags: commonTags
}

resource webApp 'Microsoft.Web/sites@2023-01-01' = {
  name: '${namePrefix}-app'
  location: location
  kind: 'app,linux'
  properties: {
    serverFarmId: appServicePlan.id
    siteConfig: {
      linuxFxVersion: 'DOCKER|mcr.microsoft.com/appsvc/staticsite:latest'
      appSettings: [
        {
          name: 'STORAGE_ACCOUNT_NAME'
          value: storageAccount.name
        }
      ]
    }
    httpsOnly: true
  }
  tags: commonTags
}

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------
output storageAccountName string = storageAccount.name
output webAppHostName string = webApp.properties.defaultHostName
output webAppId string = webApp.id
