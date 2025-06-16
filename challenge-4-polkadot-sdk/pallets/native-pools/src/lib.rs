//! NativePool pallet for managing native token deposits and daily rewards.
//!
//! This pallet allows users to deposit native tokens into a pool and receive
//! proportional daily rewards. Users can withdraw their deposits plus accumulated
//! rewards at any time. Only authorized team members can deposit rewards.

#![cfg_attr(not(feature = "std"), no_std)]

use frame::prelude::*;
use polkadot_sdk::polkadot_sdk_frame as frame;
use polkadot_sdk::frame_support::{
	traits::{Currency, ExistenceRequirement, Get},
	PalletId,
};
use polkadot_sdk::sp_runtime::{
	traits::{AccountIdConversion, Saturating, Zero}
};

// Re-export all pallet parts, this is needed to properly import the pallet into the runtime.
pub use pallet::*;

type BalanceOf<T> =
	<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

type BlockNumberFor<T> = frame_system::pallet_prelude::BlockNumberFor<T>;

/// Information about a user's deposit in the pool
#[derive(Clone, Encode, Decode, MaxEncodedLen, TypeInfo, Debug, PartialEq)]
pub struct DepositInfo<Balance, BlockNumber> {
	/// The amount deposited by the user
	pub amount: Balance,
	/// The block number when the deposit was made
	pub deposit_block: BlockNumber,
	/// The reward per share at the time of deposit (used for reward calculation)
	pub reward_debt: Balance,
}

#[frame::pallet]
pub mod pallet {
	use super::*;

	#[pallet::config]
	pub trait Config: polkadot_sdk::frame_system::Config {


		type Currency: Currency<Self::AccountId>;

		/// The pallet's account ID for holding pooled funds
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		/// The origin that can deposit rewards (team members)
		type RewardOrigin: EnsureOrigin<Self::RuntimeOrigin>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Total amount deposited in the pool by all users
	#[pallet::storage]
	#[pallet::getter(fn total_deposited)]
	pub type TotalDeposited<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	/// Total rewards accumulated in the pool
	#[pallet::storage]
	#[pallet::getter(fn total_rewards)]
	pub type TotalRewards<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	/// Accumulated reward per share (scaled by 1e12 for precision)
	#[pallet::storage]
	#[pallet::getter(fn acc_reward_per_share)]
	pub type AccRewardPerShare<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	/// Information about each user's deposit
	#[pallet::storage]
	#[pallet::getter(fn deposits)]
	pub type Deposits<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		DepositInfo<BalanceOf<T>, BlockNumberFor<T>>,
		OptionQuery,
	>;

	/// Last block when rewards were updated
	#[pallet::storage]
	#[pallet::getter(fn last_reward_block)]
	pub type LastRewardBlock<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;


	#[pallet::error]
	pub enum Error<T> {
		/// User has no deposit in the pool
		NoDeposit,
		/// Insufficient balance to deposit
		InsufficientBalance,
		/// Amount must be greater than zero
		ZeroAmount,
		/// Insufficient pool balance for withdrawal
		InsufficientPoolBalance,
		/// Arithmetic overflow occurred
		ArithmeticOverflow,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Deposit native tokens into the pool
		///
		/// The dispatch origin for this call must be _Signed_.
		///
		/// - `amount`: The amount of tokens to deposit
		#[pallet::call_index(0)]
		#[pallet::weight({10_000})]
		pub fn deposit(
			origin: OriginFor<T>,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// TODO: Implement deposit functionality
			// 1. Ensure origin is signed
			let depositor = ensure_signed(origin)?;
			// 2. Validate amount > 0 and user has sufficient balance
			ensure!(
				amount > Zero::zero(),
				Error::<T>::ZeroAmount
			);
			ensure!(
				T::Currency::free_balance(&depositor) >= amount,
				Error::<T>::InsufficientBalance
			);
			// 3. Update pool state
			Self::update_pool()?;
			// 4. Transfer tokens from user to pool account
			let pool_account = Self::account_id();
			T::Currency::transfer(
				&depositor,
				&pool_account,
				amount,
				ExistenceRequirement::KeepAlive,
			)?;
			// 5. Update or create user's deposit info with proper reward_debt
			Deposits::<T>::mutate(&depositor, |deposit_info| {
				let current_acc_reward_per_share = AccRewardPerShare::<T>::get();
				let precision = Self::precision();
				let reward_debt = amount
					.saturating_mul(current_acc_reward_per_share)
					.checked_div(&precision)
					.ok_or(Error::<T>::ArithmeticOverflow);
				
				if let Some(info) = deposit_info {
					// Update existing deposit
					info.amount = info.amount.saturating_add(amount);
					info.reward_debt = info.reward_debt.saturating_add(reward_debt);
				} else {
					// Create new deposit entry
					*deposit_info = Some(DepositInfo {
						amount,
						deposit_block: frame_system::Pallet::<T>::block_number(),
						reward_debt,
					});
				}
			});
			// 6. Update total deposited amount
			TotalDeposited::<T>::mutate(|total| {
				*total = total.saturating_add(amount);
			});
			Ok(())
			// 
			// Hints:
			// - Use Self::update_pool() before modifying state
			// - Calculate reward_debt = amount × AccRewardPerShare / precision
			// - Use Deposits::<T>::mutate() to handle existing vs new deposits
		}

		/// Withdraw tokens and rewards from the pool
		///
		/// The dispatch origin for this call must be _Signed_.
		///
		/// - `amount`: The amount of deposited tokens to withdraw (None for full withdrawal)
		#[pallet::call_index(1)]
		#[pallet::weight({10_000})]
		pub fn withdraw(
			origin: OriginFor<T>,
			amount: Option<BalanceOf<T>>,
		) -> DispatchResult {
			// TODO: Implement withdraw functionality
			// 1. Ensure origin is signed and user has deposit
			let withdrawer = ensure_signed(origin)?;
			ensure!(
				Deposits::<T>::contains_key(&withdrawer),
				Error::<T>::NoDeposit
			);
			// 2. Update pool state
			Self::update_pool()?;
			// 3. Calculate pending rewards
			let pending_rewards = Self::calculate_pending_rewards(&withdrawer)?;
			ensure!(pending_rewards > Zero::zero(), Error::<T>::NoDeposit);
			// 4. Determine withdrawal amount (use deposit amount if None)
			let withdraw_amount = match amount {
				Some(a) => a,
				None => {
					// Full withdrawal, get user's deposit info
					let deposit_info = Deposits::<T>::get(&withdrawer)
						.ok_or(Error::<T>::NoDeposit)?;
					deposit_info.amount
				}
			};
			// 5. Validate withdrawal amount and pool balance
			let total_withdrawal = withdraw_amount.saturating_add(pending_rewards);
			ensure!(
				withdraw_amount > Zero::zero() && total_withdrawal <= TotalDeposited::<T>::get(),
				Error::<T>::InsufficientPoolBalance
			);
			// 6. Update user's deposit info (remove if full withdrawal)
			Deposits::<T>::mutate(&withdrawer, |deposit_info| {
				if let Some(info) = deposit_info {
					if withdraw_amount >= info.amount {
						// Full withdrawal, remove deposit
						*deposit_info = None;
					} else {
						// Partial withdrawal, adjust deposit amount and reward debt
						info.amount = info.amount.saturating_sub(withdraw_amount);
						let precision = Self::precision();
						let current_acc_reward_per_share = AccRewardPerShare::<T>::get();
						let reward_debt = withdraw_amount
							.saturating_mul(current_acc_reward_per_share)
							.checked_div(&precision)
							.ok_or(Error::<T>::ArithmeticOverflow);
						info.reward_debt = info.reward_debt.saturating_sub(reward_debt);
					}
				}
			});
			// 7. Update total deposited
			TotalDeposited::<T>::mutate(|total| {
				*total = total.saturating_sub(withdraw_amount);
			});
			// 8. Transfer tokens + rewards back to user
			let pool_account = Self::account_id();
			T::Currency::transfer(
				&pool_account,
				&withdrawer,
				total_withdrawal,
				ExistenceRequirement::KeepAlive,
			)?;
			Ok(())
			//
			// Hints:
			// - total_withdrawal = withdraw_amount + pending_rewards
			// - For partial withdrawal, recalculate reward_debt for remaining amount
			// - Use Deposits::<T>::remove() for full withdrawal
		}

		/// Claim pending rewards without withdrawing deposit
		///
		/// The dispatch origin for this call must be _Signed_.
		#[pallet::call_index(2)]
		#[pallet::weight({10_000})]
		pub fn claim_rewards(origin: OriginFor<T>) -> DispatchResult {
			// TODO: Implement claim_rewards functionality
			// 1. Ensure origin is signed and user has deposit
			let caller = ensure_signed(origin)?;
			let claimer = caller.clone();
			ensure!(
				Deposits::<T>::contains_key(&claimer),
				Error::<T>::NoDeposit
			);
			// 2. Update pool state
			Self::update_pool()?;
			// 3. Calculate pending rewards
			let pending_rewards = Self::calculate_pending_rewards(&claimer)?;
			// 4. Validate rewards > 0 and pool has sufficient balance
			ensure!(pending_rewards > Zero::zero(), Error::<T>::NoDeposit);
			ensure!(
				pending_rewards <= TotalRewards::<T>::get(),
				Error::<T>::InsufficientPoolBalance
			);
			// 5. Update user's reward_debt to current level
			let precision = Self::precision();
			let current_acc_reward_per_share = AccRewardPerShare::<T>::get();
			let reward_debt = pending_rewards
				.saturating_mul(precision)
				.checked_div(&current_acc_reward_per_share)
				.ok_or(Error::<T>::ArithmeticOverflow)?;
			Deposits::<T>::mutate(&claimer, |deposit_info| {
				if let Some(info) = deposit_info {
					info.reward_debt = reward_debt;
				} else {
					// If no deposit exists, create a dummy entry to avoid errors
					*deposit_info = Some(DepositInfo {
						amount: Zero::zero(),
						deposit_block: frame_system::Pallet::<T>::block_number(),
						reward_debt,
					});
				}
			});
			// 6. Transfer rewards to user
			let pool_account = Self::account_id();
			T::Currency::transfer(
				&pool_account,
				&claimer,
				pending_rewards,
				ExistenceRequirement::KeepAlive,
			)?;
			Ok(())
			//
			// Hints:
			// - Only transfer rewards, keep deposit amount unchanged
			// - Update reward_debt = amount × AccRewardPerShare / precision
		}

		/// Deposit rewards into the pool (team only)
		///
		/// The dispatch origin for this call must be from `RewardOrigin`.
		///
		/// - `amount`: The amount of rewards to deposit
		#[pallet::call_index(3)]
		#[pallet::weight({10_000})]
		pub fn deposit_rewards(
			origin: OriginFor<T>,
			amount: BalanceOf<T>,
		) -> DispatchResult {
			// TODO: Implement deposit_rewards functionality
			// 1. Ensure origin is from RewardOrigin and signed
			let depositor = T::RewardOrigin::ensure_origin(origin.clone()).or_else(|_| Err(ensure_signed(origin)));
			// 2. Validate amount > 0
			ensure!(
				amount > Zero::zero(),
				Error::<T>::ZeroAmount
			);
			// 3. Update pool state
			Self::update_pool()?;
			// 4. Transfer rewards to pool account
			let pool_account = Self::account_id();
			T::Currency::transfer(
				depositor,
				&pool_account,
				amount,
				ExistenceRequirement::KeepAlive,
			)?;
			// 5. Update total rewards
			TotalRewards::<T>::mutate(|total| {
				*total = total.saturating_add(amount);
			});	
			// 6. Update AccRewardPerShare if there are deposits
			let total_deposited = TotalDeposited::<T>::get();
			if total_deposited > Zero::zero() {
				let precision = Self::precision();
				let reward_per_share_increase =
					amount.saturating_mul(precision).checked_div(&total_deposited)
						.ok_or(Error::<T>::ArithmeticOverflow)?;
				AccRewardPerShare::<T>::mutate(|acc| {
					*acc = acc.saturating_add(reward_per_share_increase);
				});
			}
			Ok(())
			//
			// Hints:
			// - Use T::RewardOrigin::ensure_origin(origin.clone())?
			// - reward_per_share_increase = amount × precision / total_deposited
			// - Only update AccRewardPerShare if total_deposited > 0
		}
	}

	impl<T: Config> Pallet<T> {
		/// The account ID of the pool
		pub fn account_id() -> T::AccountId {
			// TODO: Convert PalletId to AccountId
			// Hint: Use T::PalletId::get().into_account_truncating()
			T::PalletId::get().into_account_truncating()
		}

		/// Precision factor for reward calculations (1e12)
		fn precision() -> BalanceOf<T> {
			// TODO: Return 1e12 as BalanceOf<T>
			// Hint: 1_000_000_000_000
			1_000_000_000_000u64.saturated_into::<BalanceOf<T>>()
		}

		/// Update pool state (called before any state-changing operation)
		fn update_pool() -> DispatchResult {
			// TODO: Update the last reward block
			// 1. Get current block number
			// 2. Update LastRewardBlock storage
			// Hint: Use frame_system::Pallet::<T>::block_number()
			let current_block = frame_system::Pallet::<T>::block_number();
			LastRewardBlock::<T>::put(current_block);
			Ok(())
		}
		/// Calculate pending rewards for a user
		fn calculate_pending_rewards(who: &T::AccountId) -> Result<BalanceOf<T>, DispatchError> {
			// TODO: Implement reward calculation
			// 1. Get user's deposit info
			let deposit_info = Deposits::<T>::get(who)
				.ok_or(Error::<T>::NoDeposit)?;
			// 2. Get current AccRewardPerShare
			let current_acc_reward_per_share = AccRewardPerShare::<T>::get();
			// 3. Calculate total rewards user should have: amount × AccRewardPerShare / precision
			let precision = Self::precision();
			let total_rewards = deposit_info
				.amount
				.saturating_mul(current_acc_reward_per_share)
				.checked_div(&precision)
				.ok_or(Error::<T>::ArithmeticOverflow)?;
			// 4. Calculate pending: total_rewards - reward_debt
			let pending_rewards = total_rewards
				.saturating_sub(deposit_info.reward_debt);

			Ok(pending_rewards)
			//
			// Formula: pending = (amount × AccRewardPerShare / precision) - reward_debt
			//
			// This works because:
			// - total_rewards = what user would earn if they were here from start
			// - reward_debt = what they would have earned before they joined
			// - pending = what they actually earned since joining
		}

	}
}
