import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './WelcomeFlow.css';

interface BestModelSelection {
  model: {
    name: string;
    description: string;
    size_gb: number;
    file_name: string;
    repo_url: string;
    checksum: string;
  };
  explanation: string;
}

interface DownloadProgress {
  total_bytes: number;
  downloaded_bytes: number;
  percentage: number;
  speed_mbps: number;
  estimated_seconds_remaining: number;
  speed_mb_per_sec: number;
}

interface SecurityConfig {
  security_enabled: boolean;
  biometric_enabled: boolean;
  biometric_available: boolean;
}

interface WelcomeFlowProps {
  onComplete: () => void;
}

export function WelcomeFlow({ onComplete }: WelcomeFlowProps) {
  const [step, setStep] = useState<'welcome' | 'hardware' | 'security' | 'downloading' | 'complete'>('welcome');
  const [hardwareSummary, setHardwareSummary] = useState<string>('');
  const [bestModel, setBestModel] = useState<BestModelSelection | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string>('');

  // Security setup state
  const [securityConfig, setSecurityConfig] = useState<SecurityConfig | null>(null);
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [enableBiometric, setEnableBiometric] = useState(false);
  const [passwordError, setPasswordError] = useState('');
  const [settingUpSecurity, setSettingUpSecurity] = useState(false);

  useEffect(() => {
    const initOnboarding = async () => {
      try {
        const hardware = await invoke<string>('get_hardware_summary');
        setHardwareSummary(hardware);

        const model = await invoke<BestModelSelection>('get_best_model');
        setBestModel(model);

        // Load security config to check biometric availability
        const config = await invoke<SecurityConfig>('get_security_config');
        setSecurityConfig(config);
        // Pre-enable biometric if available
        if (config.biometric_available) {
          setEnableBiometric(true);
        }

        setStep('hardware');
      } catch (err) {
        setError(String(err));
      }
    };

    initOnboarding();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setupDownloadListener = async () => {
      unlisten = await listen<DownloadProgress>('download-progress', (event) => {
        setDownloadProgress(event.payload);
      });
    };

    if (step === 'downloading') {
      setupDownloadListener();
    }

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [step]);

  const handleContinueToSecurity = () => {
    setStep('security');
  };

  const handleSetupSecurity = async () => {
    // Validate password
    if (password.length < 8) {
      setPasswordError('Password must be at least 8 characters');
      return;
    }
    if (password !== confirmPassword) {
      setPasswordError('Passwords do not match');
      return;
    }

    setPasswordError('');
    setSettingUpSecurity(true);

    try {
      await invoke('setup_security', {
        password,
        enableBiometric,
      });

      // Clear sensitive data from state
      setPassword('');
      setConfirmPassword('');

      // Continue to download
      handleStartDownload();
    } catch (err) {
      setPasswordError(String(err));
      setSettingUpSecurity(false);
    }
  };

  const handleSkipSecurity = async () => {
    try {
      await invoke('skip_security_setup');
      handleStartDownload();
    } catch (err) {
      setError(String(err));
    }
  };

  const handleStartDownload = async () => {
    if (!bestModel) return;

    try {
      setStep('downloading');
      setError('');

      await invoke('download_model', {
        url: bestModel.model.repo_url,
        modelName: bestModel.model.file_name,
        checksum: bestModel.model.checksum || null,
      });

      await invoke('mark_model_downloaded');
      await invoke('complete_onboarding');

      setStep('complete');
    } catch (err) {
      setError(String(err));
    }
  };

  const handleFinish = () => {
    onComplete();
  };

  const formatTimeRemaining = (seconds: number): string => {
    if (seconds === 0) return 'Calculating...';

    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;

    if (minutes > 0) {
      return `${minutes} minute${minutes !== 1 ? 's' : ''} ${remainingSeconds} second${remainingSeconds !== 1 ? 's' : ''} remaining`;
    }
    return `${remainingSeconds} second${remainingSeconds !== 1 ? 's' : ''} remaining`;
  };

  const formatSize = (sizeGb: number): string => {
    return `${sizeGb.toFixed(1)} GB`;
  };

  if (error) {
    // A download that was interrupted resumes from where it left off, so the
    // primary action during download is Resume rather than a fresh start.
    const wasDownloading = step === 'downloading';
    return (
      <div className="welcome-flow">
        <div className="welcome-content error">
          <h1>{wasDownloading ? 'Download interrupted' : 'Error'}</h1>
          <p>{error}</p>
          {wasDownloading ? (
            <button onClick={handleStartDownload}>Resume download</button>
          ) : (
            <button onClick={() => setError('')}>Try Again</button>
          )}
        </div>
      </div>
    );
  }

  if (step === 'welcome') {
    return (
      <div className="welcome-flow">
        <div className="welcome-content">
          <h1>Welcome to Enklayve</h1>
          <p className="tagline">Your Private AI Assistant</p>
          <div className="loading-indicator">
            <div className="spinner"></div>
            <p>Analyzing your hardware...</p>
          </div>
        </div>
      </div>
    );
  }

  if (step === 'hardware' && bestModel) {
    return (
      <div className="welcome-flow">
        <div className="welcome-content">
          <h1>Welcome to Enklayve</h1>
          <p className="tagline">Your Private AI Assistant</p>

          <div className="hardware-info">
            <h2>System Detected</h2>
            <p className="hardware-summary">{hardwareSummary}</p>
          </div>

          <div className="model-recommendation">
            <h2>Recommended Model</h2>
            <p className="model-name">{bestModel.model.name}</p>
            <p className="model-explanation">{bestModel.explanation}</p>
            <p className="model-size">Download size: {formatSize(bestModel.model.size_gb)}</p>
          </div>

          <div className="action-buttons">
            <button className="primary-button" onClick={handleContinueToSecurity}>
              Continue
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (step === 'security') {
    return (
      <div className="welcome-flow">
        <div className="welcome-content">
          <h1>Secure Your Data</h1>
          <p className="tagline">Optional: Add encryption at rest</p>

          <div className="security-setup">
            <div className="security-info">
              <p>
                Protect your documents and conversations with AES-256-GCM encryption.
                All data will be encrypted on disk and require your password to access.
              </p>
            </div>

            <div className="password-form">
              <div className="input-group">
                <label htmlFor="password">Password</label>
                <input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder="Enter password (min 8 characters)"
                  autoComplete="new-password"
                />
              </div>

              <div className="input-group">
                <label htmlFor="confirm-password">Confirm Password</label>
                <input
                  id="confirm-password"
                  type="password"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  placeholder="Confirm password"
                  autoComplete="new-password"
                />
              </div>

              {securityConfig?.biometric_available && (
                <div className="checkbox-group">
                  <label>
                    <input
                      type="checkbox"
                      checked={enableBiometric}
                      onChange={(e) => setEnableBiometric(e.target.checked)}
                    />
                    <span>Enable Touch ID / Windows Hello for quick unlock</span>
                  </label>
                </div>
              )}

              {passwordError && (
                <p className="error-message">{passwordError}</p>
              )}
            </div>

            <div className="action-buttons">
              <button
                className="primary-button"
                onClick={handleSetupSecurity}
                disabled={settingUpSecurity || !password || !confirmPassword}
              >
                {settingUpSecurity ? 'Setting up...' : 'Enable Encryption'}
              </button>
              <button
                className="secondary-button"
                onClick={handleSkipSecurity}
                disabled={settingUpSecurity}
              >
                Skip for now
              </button>
            </div>

            <p className="skip-note">
              You can enable encryption later in Settings
            </p>
          </div>
        </div>
      </div>
    );
  }

  if (step === 'downloading') {
    return (
      <div className="welcome-flow">
        <div className="welcome-content">
          <h1>Setting up Enklayve</h1>
          <p className="tagline">Downloading your AI model</p>

          <div className="download-progress">
            {downloadProgress && (
              <>
                <div className="progress-bar-container">
                  <div
                    className="progress-bar"
                    style={{ width: `${downloadProgress.percentage}%` }}
                  ></div>
                </div>

                <div className="progress-details">
                  <p className="progress-percentage">{downloadProgress.percentage.toFixed(1)}%</p>
                  <p className="progress-speed">
                    {downloadProgress.speed_mb_per_sec.toFixed(1)} MB/s
                  </p>
                </div>

                {downloadProgress.estimated_seconds_remaining > 0 && (
                  <p className="time-remaining">
                    {formatTimeRemaining(downloadProgress.estimated_seconds_remaining)}
                  </p>
                )}

                <p className="download-info">
                  {(downloadProgress.downloaded_bytes / (1024 * 1024 * 1024)).toFixed(2)} GB of{' '}
                  {(downloadProgress.total_bytes / (1024 * 1024 * 1024)).toFixed(2)} GB
                </p>
              </>
            )}

            {!downloadProgress && (
              <div className="loading-indicator">
                <div className="spinner"></div>
                <p>Starting download...</p>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  if (step === 'complete') {
    return (
      <div className="welcome-flow">
        <div className="welcome-content">
          <h1>All Set!</h1>
          <p className="tagline">Enklayve is ready to use</p>

          <div className="suggestions">
            <h2>Try asking:</h2>
            <ul>
              <li>Summarize this PDF for me</li>
              <li>Help me write an email</li>
              <li>Explain quantum physics</li>
              <li>Analyze this document</li>
            </ul>
          </div>

          <div className="action-buttons">
            <button className="primary-button" onClick={handleFinish}>
              Start Chatting
            </button>
          </div>
        </div>
      </div>
    );
  }

  return null;
}
