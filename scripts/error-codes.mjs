/**
 * Error code mappings for Soroban contracts.
 * Generated from contract error.rs files.
 */

export const errorMappings = {
  // Escrow contract errors
  escrow: {
    1: "AlreadyInitialized",
    2: "NotInitialized", 
    3: "Unauthorized",
    4: "EscrowNotFound",
    5: "AlreadyFunded",
    6: "AlreadyPaid",
    7: "AlreadyRefunded",
    8: "InvalidSplit",
    9: "InvalidAmount",
    10: "NotExpired",
    11: "InsufficientBalance",
    12: "InvalidFee",
    13: "InvalidDeadline",
    14: "TooManySponsors",
    15: "ContributionNotFound",
    16: "InvalidTarget",
    17: "InvalidTreasury",
    19: "ContractPaused",
    20: "SelfPayout"
  },
  
  // Milestones contract errors
  milestones: {
    1: "AlreadyInitialized",
    2: "NotInitialized",
    3: "Unauthorized",
    4: "MilestoneNotFound",
    5: "IssueAlreadyAllocated",
    6: "IssueNotAllocated",
    7: "IssueAlreadyReleased",
    8: "OverAllocation",
    9: "InvalidSplit",
    10: "InvalidAmount",
    11: "InvalidFee",
    12: "MilestoneClosed",
    13: "TooManySponsors",
    14: "InvalidTreasury",
    15: "MilestoneAlreadyExists",
    16: "DeadlineNotPassed",
    17: "IssueNotAllocatedForDeallocate",
    18: "ContractPaused",
    19: "ContributionNotFound",
    20: "SelfPayout"
  },
  
  // Maintenance-pool contract errors
  maintenancePool: {
    1: "AlreadyInitialized",
    2: "NotInitialized",
    3: "Unauthorized",
    4: "PoolNotFound",
    5: "TokenMismatch",
    6: "InvalidAmount",
    7: "InsufficientBalance",
    8: "InvalidFee",
    9: "InvalidTreasury",
    10: "InactivityWindowNotElapsed",
    11: "NotDepositSponsor",
    12: "DepositNotFound",
    13: "ContractPaused",
    14: "DepositCountOverflow"
  }
};

/**
 * Extract numeric error code from Soroban error object.
 * @param {any} error - The error object from Soroban RPC
 * @returns {number|null} - The extracted error code, or null if not found
 */
export function extractErrorCode(error) {
  if (!error) return null;
  
  // Try to parse error from various possible structures
  
  // Case 1: Error might be a string containing the error
  if (typeof error === 'string') {
    // Look for common Soroban error patterns
    // Pattern 1: "Send failed: {...}" or "Tx failed: {...}" from submit.mjs
    const sendFailedMatch = error.match(/Send failed:\s*({.*})/);
    const txFailedMatch = error.match(/Tx failed:\s*({.*})/);
    
    if (sendFailedMatch || txFailedMatch) {
      try {
        const jsonStr = sendFailedMatch ? sendFailedMatch[1] : txFailedMatch[1];
        const obj = JSON.parse(jsonStr);
        return extractErrorCode(obj);
      } catch (e) {
        // If JSON parsing fails, fall back to general number extraction
      }
    }
    
    // Pattern 2: Look for error codes in strings like "error: 5" or "code: 5"
    const match = error.match(/error\s*code\s*[:=]?\s*(\d+)/i) || 
                  error.match(/\bcode\s*[:=]?\s*(\d+)/i) ||
                  error.match(/\berror\s*[:=]?\s*(\d+)/i) ||
                  error.match(/\bvalue\s*[:=]?\s*(\d+)/i) ||
                  error.match(/\b(\d+)\b/);
    if (match) {
      return parseInt(match[1], 10);
    }
    return null;
  }
  
  // Case 2: Error might be an object with errorResult
  if (error.errorResult) {
    return extractErrorCode(error.errorResult);
  }
  
  // Case 3: Error might have a resultXdr field (common in Soroban)
  if (error.resultXdr) {
    try {
      // Try to parse XDR to extract error code
      // This is a simplified approach - actual XDR parsing would be more complex
      const str = JSON.stringify(error.resultXdr);
      const match = str.match(/\b(\d+)\b/);
      if (match) {
        return parseInt(match[1], 10);
      }
    } catch (e) {
      // Ignore XDR parsing errors
    }
  }
  
  // Case 4: Error might have a value or code property
  if (error.code !== undefined) {
    const code = error.code;
    if (typeof code === 'number') return code;
    if (typeof code === 'string') {
      const num = parseInt(code, 10);
      if (!isNaN(num)) return num;
    }
  }
  
  if (error.error !== undefined) {
    const err = error.error;
    if (typeof err === 'number') return err;
    if (typeof err === 'string') {
      const num = parseInt(err, 10);
      if (!isNaN(num)) return num;
    }
  }
  
  if (error.value !== undefined) {
    const value = error.value;
    if (typeof value === 'number') return value;
    if (typeof value === 'string') {
      const num = parseInt(value, 10);
      if (!isNaN(num)) return num;
    }
  }
  
  // Case 5: Check error message for numeric codes
  if (error.message) {
    return extractErrorCode(error.message);
  }
  
  // Case 6: Try to stringify and look for numbers
  try {
    const str = JSON.stringify(error);
    // Look for patterns like "error": 5 or "code": 5
    const jsonMatch = str.match(/"error"\s*:\s*(\d+)/) || 
                      str.match(/"code"\s*:\s*(\d+)/) ||
                      str.match(/"value"\s*:\s*(\d+)/);
    if (jsonMatch) {
      return parseInt(jsonMatch[1], 10);
    }
    
    // General number extraction as fallback
    const match = str.match(/\b(\d+)\b/);
    if (match) {
      return parseInt(match[1], 10);
    }
  } catch (e) {
    // Ignore JSON stringify errors
  }
  
  return null;
}

/**
 * Decode error code to human-readable names.
 * @param {number} errorCode - The numeric error code
 * @returns {Array<string>} - Array of possible error meanings across contracts
 */
export function decodeErrorCode(errorCode) {
  if (errorCode === null || errorCode === undefined) return [];
  
  const meanings = [];
  
  for (const [contractName, mapping] of Object.entries(errorMappings)) {
    if (mapping[errorCode]) {
      meanings.push(`${contractName}: ${mapping[errorCode]}`);
    }
  }
  
  return meanings;
}

/**
 * Format error with decoded meanings if available.
 * @param {any} error - The original error object
 * @returns {string} - Formatted error message with decoding
 */
export function formatError(error) {
  const errorCode = extractErrorCode(error);
  const decoded = decodeErrorCode(errorCode);
  
  let message = '';
  
  // Include the original error for debugging
  if (typeof error === 'string') {
    message = `Error: ${error}`;
  } else if (error && error.message) {
    message = `Error: ${error.message}`;
  } else {
    message = `Error: ${error}`;
  }
  
  if (errorCode !== null) {
    message += `\n\nDetected error code: ${errorCode}`;
    
    if (decoded.length > 0) {
      message += `\nPossible contract error meanings:`;
      decoded.forEach(meaning => {
        message += `\n  • ${meaning}`;
      });
      
      // Add a note about ambiguous codes
      if (decoded.length > 1) {
        message += `\n\nNote: Error code ${errorCode} exists in multiple contracts. `;
        message += `The actual meaning depends on which contract was invoked.`;
      }
    } else {
      message += `\nNo known mapping for error code ${errorCode}`;
    }
  } else {
    message += `\n\nNo error code detected. This may be a network or authentication error.`;
  }
  
  return message;
}