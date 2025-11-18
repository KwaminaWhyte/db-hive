import { FC, useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  ConnectionProfile,
  DbError,
  getDriverDisplayName,
} from '../types/database';

interface ConnectionListProps {
  /** Callback when a profile is selected for editing */
  onEdit?: (profile: ConnectionProfile) => void;
  /** Callback when profiles list changes */
  onProfilesChange?: () => void;
}

export const ConnectionList: FC<ConnectionListProps> = ({
  onEdit,
  onProfilesChange,
}) => {
  const [profiles, setProfiles] = useState<ConnectionProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [passwordPrompt, setPasswordPrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);
  const [password, setPassword] = useState('');

  // Load profiles on mount
  useEffect(() => {
    loadProfiles();
  }, []);

  // Load connection profiles
  const loadProfiles = async () => {
    setLoading(true);
    setError(null);

    try {
      const result = await invoke<ConnectionProfile[]>(
        'list_connection_profiles'
      );
      setProfiles(result);
    } catch (err) {
      const dbError = err as DbError;
      setError(`Failed to load profiles: ${dbError.message}`);
    } finally {
      setLoading(false);
    }
  };

  // Handle connect button click
  const handleConnectClick = (profile: ConnectionProfile) => {
    setPasswordPrompt({
      profileId: profile.id,
      profileName: profile.name,
    });
    setPassword('');
  };

  // Handle connect with password
  const handleConnect = async () => {
    if (!passwordPrompt) return;

    setConnectingId(passwordPrompt.profileId);
    setError(null);

    try {
      await invoke<string>('connect_to_database', {
        profileId: passwordPrompt.profileId,
        password,
      });

      // Success - close password prompt
      setPasswordPrompt(null);
      setPassword('');
      alert(`Connected to ${passwordPrompt.profileName} successfully!`);
    } catch (err) {
      const dbError = err as DbError;
      setError(`Failed to connect: ${dbError.message}`);
    } finally {
      setConnectingId(null);
    }
  };

  // Handle delete
  const handleDelete = async (profileId: string, profileName: string) => {
    if (
      !confirm(
        `Are you sure you want to delete the connection "${profileName}"?`
      )
    ) {
      return;
    }

    setError(null);

    try {
      await invoke('delete_connection_profile', { profileId });
      await loadProfiles();
      onProfilesChange?.();
    } catch (err) {
      const dbError = err as DbError;
      setError(`Failed to delete profile: ${dbError.message}`);
    }
  };

  if (loading) {
    return (
      <div style={{ padding: '20px' }}>
        <h2>Connections</h2>
        <p>Loading...</p>
      </div>
    );
  }

  return (
    <div style={{ padding: '20px' }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: '20px',
        }}
      >
        <h2>Connections</h2>
        <button
          onClick={loadProfiles}
          style={{
            padding: '8px 16px',
            fontSize: '14px',
            cursor: 'pointer',
            backgroundColor: '#6c757d',
            color: 'white',
            border: 'none',
            borderRadius: '4px',
          }}
        >
          Refresh
        </button>
      </div>

      {error && (
        <div
          style={{
            marginBottom: '15px',
            padding: '10px',
            backgroundColor: '#fee',
            color: '#c33',
            borderRadius: '4px',
          }}
        >
          {error}
        </div>
      )}

      {profiles.length === 0 ? (
        <p style={{ color: '#666' }}>
          No connections yet. Create one using the form on the right.
        </p>
      ) : (
        <div>
          {profiles.map((profile) => (
            <div
              key={profile.id}
              style={{
                marginBottom: '15px',
                padding: '15px',
                border: '1px solid #ddd',
                borderRadius: '8px',
                backgroundColor: '#f9f9f9',
              }}
            >
              <div style={{ marginBottom: '10px' }}>
                <h3 style={{ margin: '0 0 5px 0', fontSize: '18px' }}>
                  {profile.name}
                </h3>
                <div style={{ fontSize: '14px', color: '#666' }}>
                  <div>
                    <strong>Driver:</strong> {getDriverDisplayName(profile.driver)}
                  </div>
                  <div>
                    <strong>Host:</strong> {profile.host}:{profile.port}
                  </div>
                  <div>
                    <strong>Username:</strong> {profile.username}
                  </div>
                  {profile.database && (
                    <div>
                      <strong>Database:</strong> {profile.database}
                    </div>
                  )}
                </div>
              </div>

              <div style={{ display: 'flex', gap: '10px' }}>
                <button
                  onClick={() => handleConnectClick(profile)}
                  disabled={connectingId !== null}
                  style={{
                    padding: '8px 16px',
                    fontSize: '14px',
                    cursor:
                      connectingId !== null ? 'not-allowed' : 'pointer',
                    backgroundColor: '#28a745',
                    color: 'white',
                    border: 'none',
                    borderRadius: '4px',
                  }}
                >
                  {connectingId === profile.id ? 'Connecting...' : 'Connect'}
                </button>

                <button
                  onClick={() => onEdit?.(profile)}
                  style={{
                    padding: '8px 16px',
                    fontSize: '14px',
                    cursor: 'pointer',
                    backgroundColor: '#ffc107',
                    color: '#333',
                    border: 'none',
                    borderRadius: '4px',
                  }}
                >
                  Edit
                </button>

                <button
                  onClick={() => handleDelete(profile.id, profile.name)}
                  style={{
                    padding: '8px 16px',
                    fontSize: '14px',
                    cursor: 'pointer',
                    backgroundColor: '#dc3545',
                    color: 'white',
                    border: 'none',
                    borderRadius: '4px',
                  }}
                >
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Password Prompt Modal */}
      {passwordPrompt && (
        <div
          style={{
            position: 'fixed',
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            backgroundColor: 'rgba(0, 0, 0, 0.5)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            zIndex: 1000,
          }}
          onClick={() => setPasswordPrompt(null)}
        >
          <div
            style={{
              backgroundColor: 'white',
              padding: '30px',
              borderRadius: '8px',
              minWidth: '400px',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <h3 style={{ marginTop: 0 }}>
              Connect to {passwordPrompt.profileName}
            </h3>

            <div style={{ marginBottom: '20px' }}>
              <label
                htmlFor="connect-password"
                style={{ display: 'block', marginBottom: '5px' }}
              >
                Password:
              </label>
              <input
                type="password"
                id="connect-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    handleConnect();
                  }
                }}
                style={{
                  width: '100%',
                  padding: '8px',
                  fontSize: '14px',
                  boxSizing: 'border-box',
                }}
                placeholder="Enter password"
                autoFocus
              />
            </div>

            <div style={{ display: 'flex', gap: '10px', justifyContent: 'flex-end' }}>
              <button
                onClick={() => setPasswordPrompt(null)}
                style={{
                  padding: '8px 16px',
                  fontSize: '14px',
                  cursor: 'pointer',
                  backgroundColor: '#6c757d',
                  color: 'white',
                  border: 'none',
                  borderRadius: '4px',
                }}
              >
                Cancel
              </button>

              <button
                onClick={handleConnect}
                disabled={!password || connectingId !== null}
                style={{
                  padding: '8px 16px',
                  fontSize: '14px',
                  cursor:
                    !password || connectingId !== null
                      ? 'not-allowed'
                      : 'pointer',
                  backgroundColor: '#28a745',
                  color: 'white',
                  border: 'none',
                  borderRadius: '4px',
                }}
              >
                {connectingId ? 'Connecting...' : 'Connect'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
